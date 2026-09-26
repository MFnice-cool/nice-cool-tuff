# 06. Роль ИИ в TuffBox

## Главный принцип

ИИ не является dependency resolver. ИИ не должен сам решать, какие версии модов ставить, удалять или обновлять.

ИИ — это помощник для анализа логов, объяснений и генерации гипотез. Исполняет изменения только детерминированный код лаунчера после подтверждения пользователя.

## Четыре контура (не смешивать контракты)

| Контур | Контракт | UI | Источник |
|--------|----------|-----|----------|
| **Create Mode** | `PackBrief` → `PackDraft` | Sidebar **Chats** | LLM + catalog assemble |
| **Ideas** | `{ slug, count, name? }` | Content modal | Co-occurrence graph (**не** LLM) |
| **Crash diagnose** | `ActionPlan` | IDE **Diagnose** | LLM / KB / swarm lookup |
| **Tune Config Advisor** | `ActionPlan` (`edit_config` only) | IDE **Tune** AI sidebar | LLM + local hints + allowlisted web research |

Общая грамматика для игрока:

```text
Context → Proposal → Review (чекбоксы / risk) → Confirm → validate → snapshot → apply → verify / feedback
```

- ИИ / граф **предлагает**; игрок **решает**; лаунчер **исполняет**.
- Create Mode **не** эмитит ActionPlan; Ideas **не** зовёт crash planner; Tune Advisor **не** ставит/удаляет моды.
- Review UI обязателен: ActionPlanReviewPanel / TunePlanReview / DraftConfirmPanel / Ideas checkboxes — **не** silent `window.confirm` как единственный шаг (confirm может остаться только для Test-launch prompt).

## Dual-mode диагностика крашей

Настройка `ai.diagnoseMode` (Settings → Crash KB):

| Режим | Поведение |
|-------|-----------|
| **`server`** (default) | `POST /v1/crash/diagnose` на `ai.crashKbEndpoint` — сервер матчит приватную KB ± LLM, клиенту только `ActionPlan`. Без endpoint → локальный LLM (как раньше). |
| **`local`** | `POST /v1/crash/lookup` → top-N similar cases в prompt → Ollama / openai-compatible → тот же `ActionPlan`. |
| **`kb_only`** | Только matched case → plan из `actions` кейса, без LLM. |

При `swarm.enabled` + `p2p_enabled` между сильным KB/capsule short-circuit (L1) и LLM (L3)
вставляется **L2 Fog volunteer** (см. Crash Diagnose Cascade в [`13-tuffswarm-network.md`](13-tuffswarm-network.md)).
Режим `kb_only` остаётся без L2/L3 LLM.

**L3 draft→verify (opt-in):** `ai.speculativeDecoding` + `ai.draftModel` (Settings → AI).
Маленькая локальная draft-модель предлагает ActionPlan JSON → основная модель валидирует/переписывает.
Не token-level `llama-cpp-2` (это отдельный будущий cargo feature); не в Fog node.

Приватный корпус KB **никогда не шипится** в лаунчер. Builtin seed в клиенте — тонкий offline fallback.

```text
Crash logs
→ local fingerprint + inventory
→ L1 strong KB/capsule hit? → ActionPlan
→ else L2 Fog volunteer (opt-in P2P)? → ActionPlan
→ else DiagnoseMode router (L3 server/local/heuristics) → ActionPlan
→ validate → UI confirm → snapshot → apply
```

L3 Explain prompt includes Diagnose **group-test covering** and decoded **player-trail covering** as launcher facts (not raw enable/disable journal). Priority: verified group test > trail covering > culprits/KB.

API (ваш сервер):

- `POST /v1/crash/lookup` — fingerprint → hits (`solution`, `actions`, score); без `notes` / полного корпуса
- `POST /v1/crash/diagnose` — context → готовый `ActionPlan`
- Auth: bearer token (`crash_kb` в keyring); **нет** bulk dump KB

## Что делает ИИ

- объяснение stacktrace;
- гипотезы и ранжирование подозрительных модов;
- предложение **структурированного** плана (`ActionPlan`);
- помощь с config values (через `edit_config`) — контур **Tune Config Advisor** во вкладке Tune;
- предложение **квестовых глав** (`QuestPlan`) для FTB Quests editor.

## Что делает код

- граф зависимостей, metadata, версии, loader/side checks;
- snapshots, apply, rollback, export, launch;
- парсинг логов / stacktrace / fingerprint;
- валидация и применение `ActionPlan`;
- парсинг / валидация / merge `QuestPlan` → `QuestBook` (Save пишет SNBT).

## Что ИИ не должен делать напрямую

- молча удалять / обновлять моды;
- менять loader;
- переписывать configs без diff / confirm;
- применять fixes без snapshot;
- принимать окончательное решение вместо пользователя.

## System prompt (канон)

Текст живёт в `tuffbox_core::action_plan::ACTION_PLAN_SYSTEM_PROMPT` — один и тот же для server и local.

**AI Decision making** (порядок рассуждения перед JSON):

1. **Understand the context** — только shared info из промпта (MC/loader/Java, inventory, **group-test covering**, **player-trail covering**, culprits, findings, KB, graph, excerpts). Приоритет фактов: verified group test > trail covering > culprits/KB. Не сжимать covering из нескольких модов в один «главный».
2. **Isolate the problem** — одна primary root cause; ранний hard failure важнее cascading noise.
3. **Accept the risk** — у каждого action явный `risk`; `needsUserReview` / `confidence` честные.
4. **Map decision** — минимальный набор `actions` с `op`, reason ↔ isolated cause.

```text
You are TuffBox Crash Planner. You only output ONE JSON object matching schemaVersion 1.
You do NOT apply fixes. You propose an ActionPlan for the launcher.
… (см. код: decision steps + hard rules)
```
## Формат ответа ИИ — ActionPlan

Единственный executable контракт (`schemaVersion: 1`):

```json
{
  "schemaVersion": 1,
  "humanExplanation": "Игра вылетела во время инициализации рендера. Вероятно, конфликтуют Oculus и Embeddium.",
  "confidence": 0.78,
  "suspectedMods": ["oculus", "embeddium"],
  "needsUserReview": true,
  "source": "hybrid",
  "matchedCaseIds": ["case-oculus-embeddium"],
  "actions": [
    {
      "op": "update_mod",
      "modId": "oculus",
      "version": "1.7.0",
      "reason": "Версия 1.6.x часто конфликтует с текущим Embeddium.",
      "risk": "medium"
    },
    {
      "op": "install_mod",
      "modId": "indium",
      "provider": "modrinth",
      "reason": "Missing dependency for Sodium",
      "risk": "low"
    },
    {
      "op": "edit_config",
      "path": "config/example.toml",
      "patchType": "toml_set",
      "patch": { "section.key": "value" },
      "reason": "Disable conflicting feature",
      "risk": "low"
    }
  ]
}
```

Допустимые `op`: `install_mod`, `remove_mod`, `disable_mod`, `update_mod`, `change_mod_version`, `reinstall_mod`, `edit_config`.

Legacy `recommended_actions` / `action_type` ещё парсятся и нормализуются в `op`.

## Валидация и apply

```text
ActionPlan
→ validate_action_plan (unknown op = reject)
→ ActionPlanReviewPanel (per-action checkbox; needsUserReview → ack required)
→ user confirms Apply
→ snapshot
→ apply_action_plan → FixAction / ChangeAction / edit_config patch
→ test run / soft-verify
```

`edit_config.patchType`: `json_merge` | `toml_set` | `properties_set` | `replace_file`.

## Diagnose triage checklist (author)

1. Prefer **crash-report** → else `latest.log` / `launcher.log` / `hs_err_pid*.log`.
2. Read **Evidence** (signal groups + sections) and **Suspected mods** before AI.
3. Watch **cascading / OOM / mixin / coords** cards — they often beat a noisy stacktrace.
4. **Review → Apply** ActionPlan (snapshot first). Rules/heuristic Fix plan is separate.
5. For player tickets: **Support pack** or drop/import their crash / mclo.gs URL.
6. Optional: Class finder / MCreator tools; **Save KB** after a verified fix.

## Как расширять приватную Crash KB (авторский workflow)

### В IDE (удобно)

1. Открой Diagnostics → выбери crash-report.
2. (Опционально) **AI explain** / **Apply plan** — чтобы подтянуть solution и actions.
3. Нажми **Save KB case** (или «Save as KB case» под AI).
4. Заполни **Solution**, поправь **Actions JSON**, при желании **Notes** (только для тебя).
5. **Save case** → пишется в `.tuffbox/crash_kb/cases.jsonl` (`source: authored`) и публичный файл в `.tuffbox/crash_kb/export/<id>.json` **без notes**.
6. **Copy export JSON** / **Open export folder** — залей export на свой сервер KB.

Fingerprint подставляется автоматически из текущего краша.

### Вручную / на сервере

1. Кейсы храните **на сервере** (JSONL/DB), не в релизе лаунчера.
2. Каждый кейс минимум:
   - `id`, fingerprint / matchRules
   - `solution` (текст для UI/RAG)
   - `actions[]` уже в executable `op`-формате
   - `notes` / внутренние правила — **не отдавать** клиенту
3. Сильный матч → `kb_only` / diagnose без LLM.
4. Opt-in «Helped» пишет в локальный project JSONL; авторский кейс — через форму выше.

Пример кейса (как в export):

```json
{
  "id": "mixin-create-flywheel",
  "fingerprint": { "exception": "MixinTransformerError", "key": "..." },
  "solution": "Update Create and Flywheel to matching versions for this MC.",
  "actions": [
    { "op": "update_mod", "modId": "create", "version": null, "reason": "Latest compatible", "risk": "medium" }
  ]
}
```

## Network evolution (TuffSwarm)

Локальный AI и ActionPlan — **foundation**. Поверх них развивается сеть **TuffSwarm** (канон: [`13-tuffswarm-network.md`](13-tuffswarm-network.md)):

| Контур | Роль относительно этого документа |
|--------|-----------------------------------|
| Knowledge Torrent | Обмен `ExperienceCapsule` (= fingerprint + solution + `actions[]` ActionPlan), не raw logs |
| Creation Marketplace | Чужие GPU генерируют артефакты; заказчик верифицирует; награда = Kudos / premium |

**Инварианты не меняются:**

- ИИ / swarm **MUST NOT** применять фиксы без confirm + snapshot.
- Словарь действий — только executable `op` из ActionPlan (`schemaVersion: 1`).
- Author `notes` **MUST NOT** уходить в сеть.
- Dual-mode diagnose (`server` / `local` / `kb_only`) остаётся Phase B (HTTP remote KB); P2P — Phase C+.

**Два AI-контура (не смешивать):**

| Контур | Роль |
|--------|------|
| **AI Explain** | Диагностика текущего краша; может **читать** сеть (lookup/diagnose); **не** публикует капсулы |
| **Resolution Distill** | После verified fix: **group test** — `disable_mod` на isolated covering; **player trail** — decode (healthy ⇒ enabled чистые), шарить covering без выдуманного корня; **встроенный ИИ** — можно сжать в минимальный план → **Confirm/Edit** → только тогда publish |

Authored export из Diagnostics («Save KB case») — прямой вход в capsule format для будущей сети.

## Tune Config Advisor

Отдельный контур во вкладке **Tune** (сайдбар Config AI). Промпт: `TUNE_CONFIG_SYSTEM_PROMPT` в `tuffbox_core::tune_config_ai`.

```text
Goal / chat
→ TuneContext (inventory, open file, key hints from comments, deterministic templates)
→ LLM draft ActionPlan (edit_config only) + unknownKeys / researchQueries
→ optional allowlisted web research (Settings → AI → tuneWebResearch)
→ refine → validate_tune_action_plan
→ TunePlanReview (checkboxes + dry-run diff) → Confirm → apply_action_plan → snapshot
```

Правила:

- Только `op: edit_config` (`json_merge` | `toml_set` | `properties_set` | `replace_file`).
- Неизвестный ключ **не** выдумывается: research (Modrinth / wiki.gg / GitHub / …) или остаётся в `unknownKeys`.
- Silent rewrite запрещён — всегда Review.
- Optimize Pack «Use AI for configs» вызывает тот же advisor (goal `fps_client`).

### Ручной чеклист

1. Tune → AI → **Explain file** на открытом toml без сети — explanation, `actions=[]`.
2. **FPS** chip — plan с `edit_config`, Review показывает diff, Apply → snapshot, буфер обновляется.
3. **Fill unknowns** на редком моде — researchLog в UI; Settings → выключить web research → skip lookup.
4. Rollback через Snapshots после Apply.

## Create Mode (PackBrief → PackDraft)

Sidebar **Chats**: игрок описывает сборку → ИИ (или Quick assemble) предлагает `PackBrief` → лаунчер собирает `PackDraft` поиском по каталогам (версии / `file_id` / SHA выбирает **код**, не LLM).

### Фазы пайплайна

```text
Intent → Catalog → Rank (optional) → Curate loop (pillars + co-occur) → InstallPreview → Confirm → Snapshot → install_pack_draft
→ Open Content / Resolve
```

| Фаза | Кто | Контракт / данные |
|------|-----|-------------------|
| **Intent** | LLM (или Quick heuristic) | `{ reply, search, brief }` — `search.{loader,version,theme,keywords}` + `brief` (бюджеты `categories[]`, `mustHave[]`, `exclude[]`). Маркетинговые «categories» = `brief.categories[]` + `search.keywords`. |
| **Catalog** | Launcher HTTP | Modrinth primary + CurseForge fallback/bridge → `PackDraft` / candidate pool (slug, name, desc — **не** jar URL). Progress: `phase=catalog`. |
| **Rank** | LLM (опционально; Quick без Rank) | Кандидаты обратно в prompt (`CREATE_MODE_REFINE_PROMPT`) → только slugs / mustHave / exclude. Никогда URL / `file_id` / SHA. |
| **Curate** | Launcher + LLM Reviewer | `curate_pack_loop`: gameplay **pillars** из brief/NL; co-occurrence priors из Supabase (`partners_for_mod` + MPI graph) + local trends; compact cards; verdict JSON; `launcher_score` владеет best; `is_complete` запрещён при unmet priority-1. Stop: `complete` / `stuck` / `pillars_unmet` / `max_iterations`. |
| **InstallPreview** | Launcher | Resolve совместимых версий + hashes (MR/CF metadata) + dest path + skip-if-installed. |
| **ConfirmInstall** | User + Launcher | DraftConfirmPanel → `confirmed: true` → snapshot → `install_pack_draft`. |

```text
Описание
→ Intent (PackBrief / Quick)
→ Catalog (assemble_pack_draft)
→ Rank with AI (optional) / Curate (recommended)
→ InstallPreview (preview_pack_draft)
→ DraftConfirmPanel
→ confirm → snapshot → install_pack_draft
→ Open Content / Resolve
```

**Curation invariants**

- Pillars first: QoL/performance-only packs are a failure even without conflicts.
- Launcher recomputes pillar coverage; LLM `coverage_score` is tie-break only.
- Role caps: performance+library ≲15%, support ≲20% of `targetCount`.
- Co-occurrence prior даёт **slugs** для поиска/mustHave — не jar URL / `file_id`.
- Per-iteration: keyword catalog (cached) → cheap `graph_hints` (duplicate / overload / common deps) → Reviewer.
- Potato PC (`launcher_settings.potato_pc`): default maxIter=3, 24 cards, no SearchRole LLM, lighter hints, 120s budget.
- Stop reasons: `complete` | `stuck` | `pillars_unmet` | `max_iterations` | `timeout` | `cancelled` | `empty_pool` | `ai_down`.
- Cancel: `cancel_curate_pack_loop` (cooperative, between iterations).
- `rank_pack_draft` = `curate_pack_loop(..., maxIterations=1)` (thin alias).
- Session persist: `CreateChatSession.curation` restores pillar checklist after reload.
- Core: `tuffbox_core::create_mode_curation`; command: `curate_pack_loop`.

Жёсткие правила:

- ИИ **не** эмитит ActionPlan и **не** выбирает `file_id` / jar URL / checksums.
- Install только после явного Confirm (`confirmed: true`).
- «Песочница» install = project mods path + provider download + snapshot rollback (отдельный FsScope API не требуется).
- Creation Marketplace / remote GPU — отдельный Phase D, не этот путь.

## Ideas (Often installed together)

После Add-mod в Content: граф co-occurrence (Supabase / MPI / local pairs) предлагает companions. Это **не** LLM.

```text
Install mod X
→ suggest_partners_for_mod (compat-filtered)
→ Ideas modal (name/icon/count)
→ Install selected → snapshot via add_*_with_dependencies
→ missing-deps / Resolve
```

Dismiss пишет soft-blocklist в localStorage (без сети).

## Формат ответа ИИ — QuestPlan (FTB Quests)

Второй executable контракт (`schemaVersion: 1`), рядом с ActionPlan. Канон и system prompt: `tuffbox_core::quest_plan`.

```text
Prompt / model JSON
→ parse_quest_plan (strips ``` fences)
→ validate_quest_plan
→ merge_quest_plan into QuestBook (memory)
→ UI confirm → Save chapter SNBT → play in game
```

ИИ **не** пишет `.snbt` сам. Лаунчер понимает declarative `chapters[]` (upsert/replace), генерирует hex id, резолвит `dependencies` по id **или** точному title. Опционально `rewardTables[]` и `chapterGroups[]`.

### Multi-pass (линии 20+)

Для больших линий (`generate_quest_line` / Quest AI chat):

1. **Outline** — titles, deps, tasks/rewards skeletons  
2. **Lore** — `description[]` чанками (3–6 строк), иначе template fill  
3. **Ground** — item ids → catalog  
4. **Layout** — DAG auto-layout (x/y)

Сессии чата: `.tuffbox/quest_chats/`. Review с чекбоксами chapter/quest → Apply в память редактора → **Save all**.

Минимальный пример:

```json
{
  "schemaVersion": 1,
  "humanExplanation": "Early Create progression",
  "confidence": 0.82,
  "needsUserReview": true,
  "chapters": [{
    "title": "Andesite Age",
    "icon": "create:andesite_alloy",
    "quests": [{
      "title": "Cobblestone",
      "x": 0, "y": 0,
      "description": ["&7Collect cobble", "&aRequired for the line"],
      "tasks": [{ "type": "item", "properties": { "item": "minecraft:cobblestone" } }]
    }]
  }]
}
```

UI: Quest editor → **AI** sidebar → multi-turn chat → Review → Apply → Save all.

Простые нумерованные запросы (как «добудь 10 дерева… награда 10 палок») разбирает офлайн-эвристика; свободный текст / «линейка на 24» — multi-pass через настроенный AI (Ollama / OpenAI-compatible).
