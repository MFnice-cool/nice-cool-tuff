План: категорийно-осознанное определение конфликтов модов и их разрешение
Почему сейчас «разрешение хромает» (диагноз)
Я разобрал текущий пайплайн: crash.rs::analyze_* → build_hints → create_crash_fix_plan, resolver.rs::create_fix_plan, action_plan.rs, каскад analyze_crash_with_ai в lib.rs, UI Diagnostics.svelte. Конкретные дыры:
1. Нет сущности «конфликтная пара». Есть список подозреваемых с рангом (BlameRole), а не (A, B, вид) из лога. create_crash_fix_plan выбирает suspected_mods.first() и предлагает disable/update для ОДНОГО мода. Второй кандидат (sodium/indium) даже не получает честного хода.
2. Нет семантики «заменяемости» (replaceability) и категории. Я добавил хардкод is_optimization_mod под один кейс; он не знает про библиотеки/API, контент, легаси, дубли, loader-моды. Форка-версии (embeddium/rubidium) и pref-пользователя не учитываются.
3. resolver.rs MOD_CONFLICT берёт related_nodes.last() — случайный выбор, без подсчёта зависимых и категории.
4. Хинт version-mismatch выдаёт updateMod для первого имени и никак не разделяет «выключить/обновить/удалить» обеих сторон.
5. ChangePlan — одноразовый выстрел: нет альтернатив (options), нет радио-выбора «какую сторону чинить», нет запоминания решения по fingerprint — при каждом перезапуске тот же крэш, тот же план.
6. AI может переврать политику (модель на L3 из Fabric-подсказки делает «Replace spb-revamped»), а у нас нет veto.
7. Нет цикла верификации: после apply нет авто-шага «второй вариант, если Test launch снова упал» (started loop exists only via verifyPrompt, без приоритезации нескольких вариантов).
1. Матрица взаимодействий модов из разных категорий
Беру категории, близкие к MOD_GROUPS во фронтенде + backend-классификатор, и для каждой пары фиксирую «типичный исход» и правильный первый ход разрешения.
#	Категория A	Категория B	Типичный сценарий
1	Optimization (sodium, indium)	Content (spb-revamped)	A breaks B (наш кейс)
2	Optimization/render	Shader/взаимоисключающиеся (optifine ↔ sodium)	mutual exclusivity
3	Content	Library/API (fabric-api, cloth)	контент требует API
4	Content	Legacy/abandoned контент (spb-revamped под другие версии)	старая сборка против стека
5	Content	Content	реальный конфликт двух аддонов
6	Library (api)	Library (супер-пример fabric-api старый)	update API вместо контента
7	Misc/мост (indium/embedding)	Optimization	старый мост ломает движок
8	Management (jei/emi/rei)	Management (дубли)	фиче-дубли
9	Optimization	Optimization	lazy мод vs движок
10	Loader mismatch (forge-jar в fabric)	любой	WrongLoader
11	Content	Worldgen/структуры	нормальный узел, ни конфликта
12	Duplicate id (2 версии)	—	старый + новый jar
Ключевой принцип разрешения (в порядке убывания приоритета):
1. Безопасность: никогда не удалять (тихое .disabled), обе стороны предлагать как варианты, дефолт — самый «заменяемый» (optimization, дубли, legacy).
2. Замена реализуемо и обратимо: Library/API → update; Content → keep; Optimization/bridge/legacy → disable.
3. Данные: число зависимых (из графа), дата последнего апдейта, known_conflicts, легаси-флаг.
4. Преференс пользователя (персистится по fingerprint): «keep optimization», «keep content» и т.д.
2. Целевая архитектура (изменяемые/новые модули)
Пайплайн становится конвейером: ConflictPair → PolicyEngine → RankedOptions → Plan+Review → Apply → VerifyLoop.
Файл	Роль
crates/tuffbox-core/src/mod_category.rs (новый)	enum ModCategory { Library, ApiCompanion, Content, Optimization, RenderCompat, Worldgen, Tech, Magic, Decor, Qol, Legacy, Duplicate, Unknown } + classify(slug,name), replaceability(side), is_legacy(), known_conflicts из knowledge/builtin.rs и т.д. (аккуратно: категории не должны храниться как строки — оставить сериализуемые).
crates/tuffbox-core/src/mod_conflict.rs (новый)	`struct ConflictPair { a: ModRef, b: ModRef, kind: ConflictKind (Breaks
crates/tuffbox-core/src/resolve.rs (новый)	Политика-движок: fn resolve(pair: &ConflictPair, ctx: &ResolveCtx) -> RankedCandidates — под „граф зависимостей + манифест + категории + preference". Возвращает список кандидатов с score, keep_reason, action (Disable/Update/Remove/Install), риск.
crates/tuffbox-core/src/crash.rs	analyze_text_for_suspects → добавить построение Conflicts; build_hints (ModVersionMismatch) → пересадить на ScoreByPolicy; create_crash_fix_plan → вызывать resolve::rank (порядок веток: policy > missing→resolver > top-suspect).
crates/tuffbox-core/src/resolver.rs	create_fix_plan::MOD_CONFLICT — брать с policy-рангом (dependents + категории), а не last().
crates/tuffbox-core/src/action_plan.rs	heuristic_plan_from_context/plan_from_* — при росте использовать классификацию и veto: если автоплан предлагает disable/update контент/либу при существующем оптимизационном кандидате — снизить ранг до «по выбору пользователя».
src-tauri/src/lib.rs	analyze_crash_with_ai — формировать контекст fixPolicy (candidates + preferred side) в prompt; после ответа — veto_if_content_vs_optimization.
crates/tuffbox-core/src/change_plan.rs	Расширить ChangePlan: options: Vec<ChangeOption> (ChangeOption { action: ChangeAction, keep: ModId, rationale, preferred: bool }). Серde backward-compatible (добавляемое поле с #[serde(default)]).
apps/tuffbox-desktop/src/components/Diagnostics.svelte	plan-review: рендер options как «Какую сторону исправить?» — радио-выбор; при preferred подсветка; сохранить fixPref[fingerprint] в localStorage (store.ts). Добавить цикл «apply → Test launch → если краш — показать next option».
apps/tuffbox-desktop/src/lib/store.ts	fixPreferenceByFingerprint: SvelteMap<string, string> + persist.
apps/tuffbox-desktop/src-tauri/src/integrations.rs	(если где настроится «keep optimization» pref) — скорее не надо.
Интерфейсы движения данных (пример) — чтобы проще понимать совместимость этапов:
// mod_category.rs
enum ModCategory { Api, Content, Optimization, Render, Legacy, Duplicate, Unknown }
fn classify(mod_id: &str, name: &str) -> ModCategory
fn replaceability(cat: ModCategory) -> u8 /* 0..100, 100 = «можно выключить» */

// mod_conflict.rs
enum ConflictKind { Breaking, DependsOn, Mixin, Duplicate, Loader, Outdated }
struct Conflict { a: String, b: String, kind: ConflictKind, reason: String }

// resolve.rs
struct ResolveCtx { graph: &DependencyGraph, manifest: &ProjectManifest, user_pref: &PreferenceStore }
struct RankedFix { action: ChangeAction, keep_mod: String, reason: String, score: f32, /*
    preferred: bool, revertible: bool, dependents_lost: usize }
fn rank(conflicts: &[Conflict], suspects: &[SuspectedMod], ctx: &ResolveCtx) -> Option<RankedFix>
3. Конкретные шаги плана (TDD, 2‑5 минут)
Task 1: ModCategory + классификатор
Files: crates/tuffbox-core/src/mod_category.rs (новый), crates/tuffbox-core/src/lib.rs (mod запись), тесты в mod_category.rs.
- Тест 1 (table) — для списка (slug,name) → категория:
("sodium", …) → Optimization, ("indium", …) → Api, ("create", …) → Tech, ("jei", …) → Content/ApiUi.
Также classify_unknown("zz-mystic") → Unknown.
- Реализация: словарь встроенных конф-условий из knowledge/builtin.rs (там уже есть category: "optimization" для sodium; подтянуть), маски regex («-fabric», «embeddium», …). Строго: постоянные категории enum, сериализация camelCase.
Task 2: парсер конфликтных пар (mod_conflict.rs)
Files: новый файл + тесты с фикстурами.
- Фикстуры для "is incompatible with any version of mod 'Sodium'", "breaks sodium", "conflicts with 'Sodium'", "NEG_HARD_DEP spb-revamped 1.2.0 {breaks indium @ [*]}", мплуриал "provides: [modA], conflicts: [modB]".
- Функции: parse_fabric_breakage(log_line) -> Option<ConflictKind::Breaking>, extract_quoted_mod_ids (уже есть) → собрать pair из строк «A is incompatible with B».
- Межете: в mod_conflict.rs только парсеры; crash.rs вызывает их в analyze_text_for_suspects/build_crash_diagnosis и кладёт conflicts в CrashDiagnosis (новое поле с #[serde(default)]).
Task 3: policy-движок resolve.rs
Files: resolve.rs + тесты кейсов 1,3,5,8 из матрицы.
- Ранжирование: score = w_repl * (100 - replaceability) + w_depend * (0 - dependents) + w_recency * (дата обновления) + user_pref;
- Огород: если kind == DependsOn и нет installed — приорити вешает Install(project) (это и так есть в resolver — интегрировать через единый rank).
- Тест кейса spb: rank([{a:spb,b:sodium,Breaking}, {a:spb, b:indium,Breaking}], suspects, ctx) → дефолт [Disable sodium, Disable indium] с preferred=true, dependents_lost=0, альтернатива [Disable spb-revamped].
- Тест: если контент (spb) имеет is_legacy=true (заброшен, старая сборка) — сместить вес на Disable legacy.
Task 4: переписать crash.rs::create_crash_fix_plan + build_hints
- create_crash_fix_plan: сохранить старую сигнатуру (ein bind «ревью» совместимость), внутри переключить порядок:
1. resolve::rank по конфлинктам;
2. pre-existing missing (resolver пока не трогаем);
3. legacy top-suspect с is_optimization_mod как первичная настройка (уже есть, но теперь через категории, не хардкод).
- build_hints для ModVersionMismatch: вместо одного updateMod-first — группа fixes для обеих сторон с лейблами (Disable {X}, Disable {Y}, Update {X}). Колонния с data.fixPlan.
- Тест: фикстура SP-Backrooms → план содержит 2 options, preferred = оптимизационным.
Task 5: resolver.rs MOD_CONFLICT
- Заменить related_nodes.last() на использовать dependents-число из graph + mod_category, выбрать отрицательный.
- Тест сборки: два конфликтных mod's, один на миксе зависимостей → правильно в сторону.
Task 6: AI-policy veto в lib.rs::analyze_crash_with_ai + heuristic_plan_from_context в action_plan
- Прокинуть conflicts и ranked в контекст; в ai_plan_with_fallback добавить пост-проверку: если план содержит RemoveMod/UpdateMod для Content/Api-мода и в конфликтной паре есть Optimization/Duplicate — отметить plan.meta.policy_vetoed=true, вернуть candidate-как пользовательский выбор, а не авто-apply.
- Тесты в action_plan (veto срабатывает, если указывать disabled).
Task 7: change_plan.rs — options + ChangeOption
#[serde(default)]
pub options: Vec<ChangeOption>,
impl ChangeOption { label, keep_mod: Option<String>, reason: String, preferred: bool }
- Backward-compat: сериализация старая остаётся (новые поля default), отсутствие полей → фронт fallback рендерит как сейчас.
- Тесты serde: без полей → default по-прежнему работает.
Task 8: фронтенд Diagnostics.svelte + store.ts
- renderOptions в plan-review: если plan.options.len > 1 → «Выбери способ исправить» radio, preferred подсвечен. Сохранение выбора → store.ts fixPreference[fingerprint] → следующий analyze_crash_with_ai/plan это учитывает (через IPC‑поле).
- Верификация-цикл: после apply вместо одного verifyBanner — alternatives[] в стейте; при провале test launch отобразить следующий option.
- Тесты: vitest-обвязка над store (езвистый store без <diag>), svelte-check.
Task 9: финальная проверка и документация
- cargo test -p tuffbox-core --lib (всь suite, — до 417 тестов в core+desktop), cargo clippy паipel-like, npx svelte-check, npm run lint:tokens (не трогаем border-radius).
- docs/14-mod-conflicts.md или ADR: описание матрици и приоритетов; manage_adr запись о политике разрешения.
4. Как это меняет «разрешение хромающее» в TODAY-кейсе
Для сборки into-the-backrooms (spb‑revamped 1.2.0 ↔ sodium 0.5.13/indium 1.0.36):
- До: план = Disable/Update spb-revamped (из построенного мода или нелепой подсказки AI «Replace»).
- После: план = Disable Sodium + Disable Indium (ukazano preferred, keeps content), альтернатива Disable SP-Backrooms, Update spb-revamped — пользователь выбирает радио, выбор запоминается localStorage по fingerprint, при повторном запуске диагностика подхватывает то же решение; если Test-запуск снова падает — циклируемся на второй вариант.