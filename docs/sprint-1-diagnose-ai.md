# Спринт 1 — Diagnose + AI: поиск ошибок и фризов

Дата: 2026-09-11
Ветка: `arena/01a08f1a-tuffbox-build-test`
Статус: реализовано, ожидает CI-прогона (`cargo test` / `cargo clippy`) — в песочнице нет Rust-тулчейна (зеркала rustup/apt недоступны), все правки проверены структурным парсером (tree-sitter-rust: множество узлов ошибок идентично базовой версии) и TypeScript-парсером для Svelte.

---

## Цель

Найти ключевые причины «медленного/зависающего» механизма Diagnose с ИИ и устранить их. Трассировке подвергся весь путь:

```
UI load() → get_crash_diagnosis → runUnifiedAnalysis → run_crash_assistant_full
          → build_ai_crash_context → ensure_ollama_model → analyze_crash_with_ai
          → каскад L1 (KB/capsule) → L2 (fog volunteer) → L3 (server/LLM) → heuristics
          → co-occurrence → persist pending plan
```

---

## Найденные корневые проблемы

### R1 — КРИТИЧНО: кэш `get_crash_diagnosis` никогда не заполнялся (ключ проблемы №1)

`apps/.../lib.rs`, `get_crash_diagnosis_impl`: в начале функции есть `api_cache::get(&cache_key)` с комментарием про «Loaded from cache», но **`api_cache::put` для `crashdiag:`-ключа отсутствовал нигде**. Быстрый путь был мёртвым кодом: каждый повторный `load()` (открытие вкладки, Refresh, смена источника лога, возврат на вкладку) гонял полный холодный анализ — чтение логов, граф, enrich, 36 правил Crash Assistant.

### R2 — КРИТИЧНО: `read_log_tail` читал файл ЦЕЛИКОМ (ключ проблемы №2)

`crates/tuffbox-core/src/process.rs`: `read_lines_lossy(reader).collect()` — все строки файла, затем отрезание хвоста. `latest.log` длинной Minecraft-сессии — сотни МБ. Вызывается **много раз за один diagnose**: `analyze_latest_log`, `analyze_launcher_log`, `analyze_debug_log`, **каждый** архивный лог упавших сессий (`load_archived_session_logs` — N файлов × полный размер), class-finder, `run_crash_assistant_analysis`, `prepare_ai_crash_context`. Итог: гигабайты чтения+парсинга на один холодный прогон — диск I/O-bound, отсюда «диагностика висит».

### R3 — Двойная тяжёлая работа на холодном пути

`prepare_ai_crash_context_uncached` **повторно** выполнял `build_crash_diagnosis` (та же повторная прогонка логов/графа) и `run_full_analysis` (36 правил), не переиспользуя результат base-диагностики. Результат: каждый холодный AI-explain делал двойной полный проход.

### R4 — «Retry storm» перед каждым AI-анализом

`ensure_ollama_ready`: 4 попытки `ollama_list_models` с HTTP-таймаутом 8с + паузы ≈ до **35с+** «Preparing local AI…» на больной/зависший daemon; плюс `ensure_ollama_daemon` → `wait_ollama_api(10)` — ещё до ~30с (10 попыток × 8с таймаут). На фронте всё это стояло **строго последовательно перед** тяжёлым `build_ai_crash_context` (`runAiExplain`). Итого десятки секунд накладных расходов на каждый AI-запрос.

Дополнительно: `ollama_binary_exists` синхронно спавнил `ollama --version` через `std::process::Command::status()` **без таймаута** — висящий бинарник на PATH блокировал весь async-executor.

### R5 — Повторные сетевые пробы и неограниченный poll в каскаде

- `capsule_transport_bases()` (внутри неё `ensure_node_running`: spawn ноды + `p2p_authorized` poll) вызывался **до 4 раз за один каскад**: в начале `analyze_crash_with_ai`, затем внутри `lookup_across_transports` (L1 и KbOnly-L1), внутри `diagnose_across_transports` (L3), плюс `ensure_node_running` в `diagnose_via_volunteer` (L2). Каждая проба при мёртвом control-plane — секунды.
- `ensure_node_running` ждал фиксированные 40 итераций × (250ms + до 2с HTTP `p2p_authorized`) ≈ **до ~90с** при недостижимом control URL.
- Итоговый worst-case каскада при мёртвой сети и включённом swarm: 20с (bases) + 12с (L1) + 12с (L2) + 12с (L3) + 12с (co-occurrence) = **~68с чистого ожидания до локального ответа** — и это ещё до Ollama. Именно это наблюдалось как «Diagnose appeared stuck on L1/L2/L3».

### R6 — Нет single-flight: повторный AI-запрос порождал второй параллельный каскад

Watchdog UI (180с) сбрасывает busy-флаги, но бэкенд-каскад продолжает работать. Повторный клик «AI explain» запускал полный второй каскад (двойная сеть, двойной CPU), а его поздний результат отбрасывался generation-guard'ом. Ресурсы и латентность — в никуда.

### R7 — Sync disk I/O и Ed25519 прямо на async-рантайме

В `analyze_crash_with_ai` (async fn) без `spawn_blocking` выполнялись: `global_capsule_library().lookup/diagnose_best` (чтение+парс всего capsульного JSONL + **Ed25519-верификация по каждому кандидату**), `top_cooccurrence_pairs` (диск), `maybe_persist_pending_from_plan` (запись на диск). Это блокировало worker'ов tokio — тормозили **все** IPC-команды, включая события UI → общее ощущение «фризов» приложения во время диагностики.

---

## Реализованные исправления

| # | Файл | Что сделано |
|---|------|-------------|
| F1 | `apps/.../lib.rs` | `get_crash_diagnosis_impl` теперь **сохраняет** результат в api_cache под тем же mtime-ключом (TTL 60с — ключ покрывает все входы, TTL только ограничивает память). «Loaded from cache» больше не мёртвый код: повторное открытие/refresh/смена источника в пределах минуты — мгновенно. |
| F2 | `crates/.../process.rs` | `read_log_tail` переписан на **обратное окно**: seek от конца файла (2 KiB/строка бюджет, min 64 KiB, cap 32 MiB), отбрасывание частичной первой строки, lossy-декодинг как раньше. O(хвост) вместо O(файл). +9 unit-тестов (базовый хвост, пустой/маленький файл, отсутствие файла, limit=0, без завершающего `\n`, частичная строка окна, multi-byte UTF-8, невалидные байты, гигантская одиночная строка). |
| F3 | `apps/.../lib.rs` | `prepare_ai_crash_context_uncached` переиспользует закэшированный `CrashDiagnosis` base-диагностики (тот же ключ) — второй полный проход логов/графа на холодном AI-пути устранён. |
| F4 | `apps/.../integrations.rs` | `ensure_ollama_ready`: ≤2 короткие пробы (3с) вместо 4×8с, старт-попытка только если бинарник реально есть; `ollama_list_models_with_timeout(root, secs)`; `wait_ollama_api` зондирует с 2с таймаутом; `ensure_ollama_daemon` — проба 2с и `wait_ollama_api(5)`; `ollama_binary_exists` — bounded (2с, kill после дедлайна). Worst-case прогрева: ~7–15с вместо ~35–90с, happy-path не изменился. |
| F5 | `apps/.../swarm_node.rs` + `lib.rs` | `lookup_across_transports`/`diagnose_across_transports` принимают предвычисленные `bases: &[String]` — одна проба транспорта на каскад вместо трёх-четырёх повторных `ensure_node_running`. |
| F6 | `apps/.../swarm_node.rs` | `ensure_node_running`: wall-clock дедлайн 8с вместо 40 итераций (теоретических ~90с) + ранний выход, если заспавненный child уже умер. |
| F7 | `apps/.../lib.rs` | **Single-flight** для `analyze_crash_with_ai`: `AI_CASCADE_INFLIGHT` (ключ `path:report_id`), конкурентные вызовы делят один `tokio::sync::OnceCell` — второй клик получает результат того же запуска вместо дубля каскада; запись вычищается после завершения. |
| F8 | `apps/.../lib.rs` | Sync I/O вынесен на blocking pool: capsule library lookup, `try_l1_strong_plan`, `top_cooccurrence_pairs`, `maybe_persist_pending_from_plan` — async-рантайм больше не блокируется диском/Ed25519. |
| F9 | `apps/.../lib.rs` | Наблюдаемость: новые `diagnose-timing` фазы `ai_l1_remote_lookup_hit`, `ai_l2_volunteer`, `ai_l3_server`, `ai_cascade_total` (cacheHit=kb_short_circuit) — автоматически появляются в Advanced → Performance phases. |
| F10 | `Diagnostics.svelte` | `ensure_ollama_model` и `build_ai_crash_context` запускаются **параллельно** (Promise), а не последовательно; сообщения статуса сохранены. |
| F11 | `Cargo.toml` | tokio-фичи `time`/`sync` объявлены явно (ранее тянулись транзитивно). |
| — | `apps/.../lib.rs` | +2 регрессионных теста: кэш диагноза (put + идентичность второго прогона) и разведение ключей по source. |

## Оценка эффекта (бюджеты до/после)

| Сценарий | До | После |
|---|---|---|
| Повторное открытие Diagnose без изменений | полный пересчёт (секунды–десятки секунд) | ≤60с TTL: мгновенно из кэша |
| Холодный base-анализ (latest.log 300МБ + 40 архивных сессий + debug.log) | полное чтение >1.5ГБ × (1…2 из-за R3) | окна хвостов (~5МБ суммарно) + переиспользование diagnosis в AI-context |
| AI-explain, больной Ollama daemon | ~35–90с прогрева последовательно + тяжёлый context build | ≤~7–15с прогрева параллельно с context build |
| Каскад при мёртвом swarm/P2P | 20+12+12+12+12с + повторные пробы ноды внутри шагов | одна проба ≤20с; шаги без внутренних перепроб; poll ноды ≤8с |
| Повторный «AI explain» во время работы каскада | второй полный каскад параллельно | результат общего запуска |
| Прочие IPC во время диагностики | блокировались sync disk/Ed25519 на рантайме | на blocking pool |

Worst-case «мёртвой сети + больного Ollama» до фиксов ≈ 3.5–5 минут; после — ≈ 60–90с и каждую секунду пользователь видит осмысленную стадию каскада. Happy-path (тёплый кэш, живая KB, рабочий Ollama) не деградировал: те же вызовы, те же семантики, только без повторной работы.

## Что осознанно НЕ менялось (бэклог спринта 2)

1. **R8:** `CapsuleLibrary::load_all` перечитывает JSONL при каждом lookup — предложить mtime-keyed parse-кэш (после F8 это больше не фризит UI, но остаётся CPU-стоимостью).
2. Настоящая отмена backend-задач (cancellation token) вместо generation-guard + single-flight; watchdog 180с оставить как последний рубеж.
3. `runUnifiedAnalysis`: базовые правила и AI по-прежнему последовательны (осознанное UX-решение — сначала быстрые findings); можно распараллелить частично (class-finder ∥ AI-context).
4. Единый `DiagnosticsSnapshot` (фаза 1 из docs/diagnostics-audit.md) — стратегическая реконструкция, не спринт-фикс.
5. Фронтовые баги из BUG_REPORT.md (Bug 1, 3–8) — вне скоупа этого спринта.

## Как проверить

```bash
cargo test -p tuffbox-core --lib process::            # новые тесты read_log_tail
cargo test -p tuffbox-desktop --lib crash_diagnosis_cache_tests
cargo clippy -p tuffbox-core -p tuffbox-desktop
```

Профиль по плану из docs/diagnose-tab-audit.md (cold/warm, события `diagnose-timing`):

1. открыть Diagnose с холодным кэшем → `base_crash_diagnosis`, `crash_assistant_rules`, `class_finder`, `ai_cascade_total`;
2. повторно открыть без изменений → `diagnosis_total` c `cache_hit=true` (<100мс);
3. выключить сеть + включить swarm → замерить `ai_l1/l2/l3` (каждый ≤12с, без повторных проб);
4. остановить Ollama → AI-путь должен упасть в heuristics за ≤~20с, не «зависая».
