# Мод-конфликты: категорийно-осознанное определение и разрешение

## Проблема (TODAY-кейс)

Для сборки into-the-backrooms (**spb-revamped 1.2.0** ↔ **Sodium 0.5.13** / **Indium 1.0.36**) старое разрешение хромало:

- план = `Disable/Update spb-revamped` (первый подозреваемый), второй кандидат (sodium/indium) не получал шанса;
- `resolver.rs::MOD_CONFLICT` брал `related_nodes.last()` — случайный выбор стороны;
- AI мог на L3 выдать «Replace spb-revamped» без veto;
- план был одноразовым — после перезапуска тот же крэш, тот же план.

Теперь: **план = Disable Sodium + Disable Indium (recommended, keeps content)**, альтернатива **Disable SP-Backrooms**. Пользователь выбирает радио, выбор запоминается по crash-fingerprint (localStorage). При повторном падении диагностика подхватывает то же решение; если Test-запуск снова упал — предлагается следующий вариант.

## Конвейер

```
ConflictPair → PolicyEngine → RankedOptions → Plan+Review → Apply → VerifyLoop
```

| Этап | Модуль |
|---|---|
| Парсинг пар | `crates/tuffbox-core/src/mod_conflict.rs` |
| Категории + заменимость | `crates/tuffbox-core/src/mod_category.rs` |
| Политика-ранг | `crates/tuffbox-core/src/resolve.rs` |
| Сборка плана | `crash.rs::create_crash_fix_plan`, `resolver.rs::create_fix_plan` |
| AI-veto | `action_plan.rs::veto_content_vs_optimization` (+ `ai_plan_with_fallback`) |
| Options в ChangePlan | `crates/tuffbox-core/src/change_plan.rs` |
| UI-выбор + память | `Diagnostics.svelte` + `lib/fixPreferences.ts` |

## Категории модов (`mod_category.rs`)

`enum ModCategory { Library, ApiCompanion, Content, Optimization, RenderCompat, Worldgen, Tech, Magic, Decor, Qol, Legacy, Duplicate, Unknown }` — сериализуемые (camelCase), никогда не строки.

`replaceability(cat) → u8` (0 = не выключать, 100 = безопасно выключить):

| Категория | replaceability | default action |
|---|---|---|
| Optimization | 90 | disable |
| Duplicate | 95 | disable |
| RenderCompat | 85 | disable |
| Legacy | 80 | disable |
| ApiCompanion | 60 | disable |
| Qol | 45 | review |
| Worldgen / Unknown | 40 | review |
| Library | 30 | update |
| Tech / Magic | 20 | keep |
| Decor | 15 | keep |
| Content | 10 | keep |

> `is_safe_to_disable = replaceability >= 60`.

## Матрица категорийных сценариев

| A | B | Типичный исход | Первый ход |
|---|---|---|---|
| Optimization (sodium, indium) | Content (spb-revamped) | A ломает B | Disable A (preferred) |
| Optimization/render | Shader-взаимоисключение (optifine↔sodium) | mutual exclusivity | Disable одного из pair |
| Content | Library/API (fabric-api, cloth) | контент требует API | Install missing / update API |
| Content | Legacy-контент | старая сборка против стека | Disable legacy |
| Content | Content | реальный конфликт двух аддонов | оба стороны в options |
| Library | (устаревший fabric-api) | update API | Update |
| Мост (indium) | Optimization | старый мост ломает движок | Disable мост |
| Management dupe (jei/emi/rei) | Management | фиче-дубли | Disable один |
| Optimization | Optimization | lazy vs движок | Disable кандидат |
| WrongLoader (forge-jar в fabric) | — | Loader | Disable / re-install |
| Worldgen | Content | нормальный узел | не конфликт |
| Два jar одной версии | — | Duplicate | Disable старый |

## Принципы разрешения (по приоритету)

1. **Безопасность**: никогда не удалять (тихий `.disabled`); обе стороны — варианты; дефолт — самый заменяемый.
2. **Заменяемость и обратимость**: Library/API → update; Content → keep; Optimization/bridge/legacy/duplicate → disable.
3. **Данные**: число зависимых (из графа), `known_conflicts`, legacy-флаг.
4. **Преференс пользователя** (persisted per fingerprint).

## Resolve score

```text
score = replaceability(cat)            // выше — охотнее выключаем
        - dependents_lost * 14        // штраф за каскад поломок
        + legacy_bonus    (если Legacy)
        + duplicate_bonus (если Duplicate)
preferred = score >= 60 && dependents_lost == 0
```

Зависимые считаются по рёбрам `Requires` в графе (`resolve::dependents_count`).

## AI-veto

`veto_content_vs_optimization(plan)` — после AI/KB/heuristic плана:
- если план авто-наменечает remove/disable/update **Content/Library**-мода, и в том же плане/подозреваемых есть **replaceable** (optimization/bridge/legacy/duplicate) — удаляет рискованное действие,
- ставит `needs_user_review = true`,
- пишет `POLICY_VETO: ...` в `additional_context`,
- возвращает выбор пользователю.

## Как проверять

```bash
cargo test -p tuffbox-core --lib                 # ~480 тестов (все зелёные)
cargo clippy -p tuffbox-core --lib
npm run check   # apps/tuffbox-desktop — новый код без новых ошибок
npm run lint:tokens
npm test        # vitest (вкл. fixPreferences.test.ts)
```