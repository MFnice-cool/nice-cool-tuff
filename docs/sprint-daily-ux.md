# Спринт: удобство базового ежедневного функционала

Дата: 2026-09-11. Проверка трёх ежедневных сценариев:

1. **Автор модпака для игрового сервера** — моды как зависимости, хотфиксы,
   client/server-разделение, экспорт `.mrpack` + server pack, публикация обновлений.
2. **Игра с друзьями на приватном сервере** — быстро поднять сервер,
   раздать пак друзьям, держать клиент и сервер в синхроне, заходить по IP.
3. **Соло-игрок на крупном сервере** — только вышла новая версия Minecraft,
   нужно быстро собрать клиентскую сборку (производительность) и играть
   на большом ванильном сервере.

## Что проверялось (по персонам)

### Персона 1 — автор серверного модпака

| Шаг | Было | Стало |
|---|---|---|
| Экспорт server pack | Только из IDE Export / Release; из Library — только `.mrpack`/Prism; с Home — никак | + Library (боковое меню и ctx-меню), + Home overflow-меню `⋯` |
| Быстрый `.mrpack` с Home | Нет (только через IDE) | + Home `⋯` → Export .mrpack (путь копируется в буфер) |
| Repair / logs.zip с Home | Нет (было в старом Dashboard-меню, потерялось при редизайне HomeHero) | + Home `⋯` → Repair, Logs .zip |
| Имена файлов экспорта | Дефолт `modpack-1.0.0.*`, т.к. `loadDefaultPaths` срабатывал до загрузки `projectInfo` (Bug 6) | Дефолты обновляются, когда приезжает реальный id/version; ручные правки не затираются |
| Update all модов | Уже есть с preview — оставлено как есть | — |
| Test → Diagnose → фикс → ретест | Next Action + work trail уже ведут — оставлено как есть | — |

### Персона 2 — приватный сервер с друзьями

| Шаг | Было | Стало |
|---|---|---|
| Список серверов / быстрый Join | **Экран `InstanceHome` (Servers/Worlds с Add/Ping/Join) вообще не смонтирован** — мёртвый код, попасть нельзя | Смонтирован на Home между hero и полкой (учитывает настройку «скрыть») |
| Вкладка Servers | Недоступна | Доступна в 1 клик; последняя вкладка запоминается |
| Run server (Test) на Linux | `defaultServerDir`/`activeLogRoot` клеились через `\`, `captureDir` переписывал `/`→`\` — битые пути | Везде `/` (корректно и на Windows) |
| Раздача пака друзьям | GitHub Pack Transport + UpdateGate уже есть — оставлено | — |

### Персона 3 — соло, свежая версия MC, крупный сервер

| Шаг | Было | Стало |
|---|---|---|
| Дефолтная версия MC | Хардкод `1.20.1` — на свежем релизе надо листать список вручную | Дефолт = latest Mojang-релиз (первая не-popular запись), в списке помечена `● latest` |
| Лоадера нет под свежую версию | Пустой список + текст ошибки | Подсказка + кнопки «Try Fabric» / «Use Vanilla» |
| Память по умолчанию | 8192 MB (много для клиентской сборки под ванильный сервер; рассинхрон с ProjectSettings/Test = 4096) | 4096 MB |
| Путь «создал → FPS-моды» | Create → IDE → Content → искать Optimize | Чекбокс «Install performance mods after creating» → авто-переход в IDE → Content → Optimize wizard |

## Файлы

- `apps/tuffbox-desktop/src/lib/store.ts` — one-shot `optimizeAfterCreate`.
- `Dashboard.svelte` — монтирование `InstanceHome`, overflow-хендлеры
  (export/repair/logs), навигация post-create в IDE.
- `HomeHero.svelte` — пункты overflow-меню (опциональные колбэки).
- `InstanceHome.svelte` — персист последней вкладки.
- `AddInstanceModal.svelte` — latest-дефолт, `● latest`, память 4096,
  loader-fallback кнопки, чекбокс Optimize.
- `Mods.svelte` — единственный потребитель `optimizeAfterCreate`.
- `LibraryInstancesPane.svelte` — `export-server` + пункты экспорта в ctx-меню.
- `ExportBuilder.svelte` — фикс Bug 6 (дефолтные пути ждут `projectInfo`).
- `TestRuns.svelte` — кроссплатформенные разделители путей.

## Проверки

- `npm run check` (svelte-check), `npm run test` (vitest),
  `npm run lint:tokens` (border-radius) — должны быть зелёными.
- Ручной сценарий 3: Blank → latest MC → Fabric → Create →
  Optimize wizard открывается сам → Apply → Play → Servers → Add → Join.
- Ручной сценарий 2: Home → `⋯` → Server pack; Test → Run server (Linux).
- Ручной сценарий 1: Content → Update all (preview) → Export → Release.

## Бэклог (найдено, не вошло)

- Bug 4 (SkinPreview3D race), Bug 5 (theme store), Bug 7 (RecipeBrowser
  double compute) из `BUG_REPORT.md` — вне ежедневного ядра, требуют
  аккуратных правок.
- Пост-create для `import`-режима: предлагать Optimize тоже (сейчас только blank).
- InstanceHome: порядок вкладок под серверных игроков (Servers первым,
  если в `servers.dat` есть записи) — сейчас просто запоминается последняя.
- `Dashboard`: `selectedLaunchMessage`/`selectedLaunching` частично не
  используются после редизайна hero — почистить при следующем проходе.
