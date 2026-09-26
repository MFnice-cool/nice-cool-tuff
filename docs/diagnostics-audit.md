# Аудит diagnostics лаунчера

Дата аудита: 2026-09-06

## Что проверено

Проверены core-путь `GraphCache → DependencyGraph → Resolver → Tauri commands`, click-path (`get_diagnostics`, `get_diagnostic_counts`, pack health), а также журнал запусков, используемый экраном Diagnose. Полный cargo-прогон в окружении невозможен: `cargo` не установлен.

## Найденные дефекты и исправления

1. **Нестабильный порядок результатов.** Missing-ноды создавались из `HashSet`, а результат резолвера зависел от порядка рёбер. Из-за этого менялись JSON, индексы действий и порядок карточек UI. Missing-ноды теперь сортируются, диагностики дедуплицируются и сортируются по severity/code/message.
2. **Дублирование карточек.** Несколько одинаковых dependency edges создавали одинаковые ошибки. Добавлена нормализация результата в одном месте перед выдачей из `Resolver`.
3. **Кэш мог блокировать диагностику.** Повреждённый или оборванный `dependency-graph.json` возвращал ошибку на warm-path. Кэш является оптимизацией, поэтому теперь read/parse failure трактуется как cache miss; диагностика продолжает работать на manifest-only графе.
4. **Перезапись кэша.** Сохранение stale cache через `NamedTempFile::persist` имело платформенную зависимость при существующем target. Используется общий atomic replacement helper.
5. **Коллизии launch history.** ID архивной сессии имел точность только до секунды. Два быстрых запуска могли записать один каталог и скрыть лог предыдущего падения. ID теперь включает миллисекунды.
6. **Неполный API конструктора.** Для симметрии с `error` и `warning` добавлен `Diagnostic::info`.

## Текущие места для оптимизации

- `diagnostics_for_click_path` повторно строит граф при каждом запросе. Следующий шаг — возвращать единый `DiagnosticSnapshot` (graph, diagnostics, fingerprint, cache source, timings) и использовать его для badge, Diagnostics и Pack Health.
- `get_pack_health_impl` повторно запускает диагностику, wrong-loader scan, duplicate scan и quest validation. Нужен один blocking pipeline с общим snapshot и параллельными независимыми фазами.
- Graph cache сейчас инвалидируется fingerprint manifest, но не имеет отдельного fingerprint файлов jar. После замены локального jar кэш может быть логически устаревшим до warm refresh. Ввести per-file `(path,size,mtime)` fingerprint или SHA-256 только для изменившихся jar.
- Resolver смешивает вычисление проблем и выбор fix plan. Разделить `Finding` (машинная причина, evidence, confidence, affected files) и `FixAction` (risk, preconditions, rollback).
- Строковые `code` и regexp-логика UI должны быть заменены стабильным enum/code registry с локализуемыми message keys. Текст сообщения не должен быть идентификатором проблемы.
- Crash/AI cascade в UI имеет watchdog, но не имеет cancellation token. Нужен `diagnose_run_id`, отмена старого run и отбрасывание late results; иначе старый AI ответ может приехать после refresh.
- Проверки содержимого и graph diagnostics не различают `manifest`, `installed jars`, `runtime log` и `crash report` как источники доказательств. Это снижает explainability и может показывать stale finding.

## Рекомендуемая реконструкция

### Фаза 1 — единый backend snapshot

Ввести `DiagnosticsSnapshot`:

- `schema_version`, `run_id`, `generated_at`;
- `source_fingerprint` и `cache_state`;
- `findings[]` с `id`, `severity`, `confidence`, `source`, `related_nodes`, `evidence`;
- `fixes[]` с preconditions и rollback metadata;
- `timings` по фазам.

Один Tauri command должен отдавать snapshot и progress events. Старые commands оставить как compatibility wrappers.

### Фаза 2 — pipeline

1. validate manifest/path;
2. load cache;
3. scan only changed jars;
4. build graph;
5. deterministic resolver;
6. filesystem/runtime/crash checks;
7. merge findings by stable key;
8. create guarded fix plan;
9. atomically persist snapshot.

Фазы 3–6 запускать параллельно на `spawn_blocking`, но merge выполнять детерминированно.

### Фаза 3 — безопасность исправлений

Каждая fix action должна проверять fingerprint до применения, создавать snapshot, применять атомарно, затем повторно запускать затронутые checks. При несовпадении fingerprint действие отклонять, а не применять к изменённому проекту.

### Фаза 4 — наблюдаемость

Записывать duration/status/error для каждой фазы, cache hit ratio, число scanned jars и размер входных логов. Не отправлять логи/AI context в сеть без явного consent. В UI показывать источник и возраст результата.

## Критерии готовности реконструкции

- одинаковый проект даёт byte-stable snapshot независимо от запуска;
- corrupted cache не ломает экран;
- повторный вызов без изменений не сканирует jars;
- смена одного jar инвалидирует только его metadata;
- late result старого `run_id` не меняет UI;
- fix после внешнего изменения проекта отклоняется;
- unit/integration tests покрывают empty pack, duplicate ids, symmetric conflicts, broken cache, rapid launches и offline mode.
