# Спринт 5 — Drag & drop импорт во вкладке «Library modpacks»

**Задача:** принимать перетаскивание prism .zip, CurseForge, Modrinth .mrpack, папки mods, zip с папкой mods, папки resourcepacks, zip с ней, shaders и zip с ними; показывать окно с предложением создать сборку из импортированного zip/rar/7z; корректно обрабатывать архивы, в которых отсутствуют некоторые папки модпака.

## Архитектура решения

Вебвью запущен с `dragDropEnabled: false` (приложение уже использует HTML5 DnD — BriefEditor, GalleryGrid), поэтому дроп приходит как DOM `File`-объекты **без путей**. Схема:

```
DOM drop → webkitGetAsEntry (рекурсивный обход папок, синхронно!)
  → чанковая загрузка байтов в Rust-стейдж (base64, 4 МиБ/чанк, прогресс)
  → inspect_import_source(path) → диалог «Создать сборку?»
  → install_modpack(source, targetDir, instanceName) → существующий пайплайн
```

- **Стейджинг:** `begin_drop_import` / `write_drop_chunk` / `finish_drop_import` / `cancel_drop_import` — временная папка `temp/tuffbox-drop-imports/…`, автоочистка старше 12ч, отмена по Escape/ошибке. Чанки пишутся по смещению (`Seek`), путь каждого файла проходит `sanitize_drop_rel` (traversal/drive-letter guard, переиспользует `is_unsafe_zip_rel_path`).
- **Диалог:** новый `DropImportDialog.svelte` (bits-ui, тот же паттерн, что PromptDialog): формат, счётчики (mods/resourcepacks/shaderpacks/files), **список отсутствующих папок** с пометкой «сборка создастся без них — это нормально для частичных паков», предупреждения, имя сборки (предзаполнено), Create/Cancel. Testids: `library-drop-overlay`, `library-drop-staging`, `library-drop-dialog`, `drop-dialog-counts`, `drop-dialog-missing`, `drop-dialog-name`, `drop-dialog-create`, `drop-dialog-cancel`.
- **Оверлей:** на всей вкладке Library, dashed-outline + стеклянная карточка «Drop to import», прогресс копирования с `role=progressbar`.

## Backend: что добавлено

| Возможность | Реализация |
|---|---|
| `.rar` / `.7z` | `normalize_foreign_pack_archive`: листинг имён → отсев unsafe-записей → извлечение (`unrar` 0.5 / `sevenz-rust` 0.6, чистый Rust) → **проверка containment** (canonicalize, защита от zip-слip в обход листинга) → переупаковка в zip → существующий sniffing-пайплайн работает без изменений. Зашифрованные 7z — понятная ошибка. |
| Content-zip (zip с папкой `resourcepacks/`, `shaders/`, `mods/`, `config/`…, без манифеста) | Раньше такие архивы проваливались в Prism-импорт и падали с «instance.cfg not found» (mods-only zip распознавался, но только с jar). Теперь: `is_content_zip_archive` → временный scaffold → `extract_content_zip` (белый список из 9 папок + корневые `.jar` → `mods/`) → повторное извлечение в финальную сборку + рескан лоадера. |
| Обёртки-папки | Zip/rar, где всё лежит в одной корневой папке (`MyPack/…`) — распознаются (`drop_wrap_prefix`) и пере-укореняются перед sniffing/извлечением. Раньше обёрнутые Prism/манифест-паки мигрировали не туда или падали. |
| Папка `resourcepacks` / `shaders` / `config` (без mods) | `stage_content_only_dir`: `import_instance_directory` требует `mods/` или `config/`; папка resourcepacks/шойдеров сама по себе не проходила. Теперь копируется в стейдж с правильной раскладкой (+пустой `mods/`), нормализацией имени (`shaders` → `shaderpacks`) и проходит обычный флоу. Голая папка `mods` идёт старым путём (без изменений). |
| `inspect_import_source` | Единый классификатор (`drop_classify_name`) по rel-именам для dir / zip / rar / 7z: формат (modrinth/curseforge/prism/packwiz/content/instance), счётчики, present/missing по 7 стандартным папкам, warnings. |

Формат детекции: `modrinth.index.json` → modrinth, `manifest.json` → curseforge, `instance.cfg`/`mmc-pack.json` → prism, `packwiz.toml` → packwiz, контентные папки/корневые jar → content.

## Верификация (прогнано в песочнице)

| Проверка | Результат |
|---|---|
| svelte compile (Library + DropImportDialog) | **0 warnings** (снапшот имени через onMount; bits-ui элементы через `:global`) |
| tree-sitter lib.rs | 12 error-узлов = **ровно baseline** (известные `&raw` FP), новых нет |
| `svelte-check` | **0 errors** (138 warnings — без изменений; поймала и исправлен `dataTransfer \| null`) |
| `vite build` | OK (~31s); 5 новых команд видны в Library-чанке |
| Гейты: border-radius / lazy-views / bundle-budget / bridge-parity | OK (193.7/230 js; bridge-parity подтверждает регистрацию всех 5 команд) |
| `vitest` | **35/35** |
| Rust unit-тесты (новые, в `drop_import_tests`) | sanitize_drop_rel (traversal/abs/drive), wrap-prefix, классификация счётчиков, content-zip детекция (вкл. обёртки и CF-ложные срабатывания), extract_content_zip (unwrap + re-root jar), stage_content_only_dir — **компиляция и прогон в CI** (тулчейна в песочнице нет) |

Риски Rust (компиляция непроверяема локально): API `unrar`/`sevenz-rust` сверены с docs.rs (0.5.8 / 0.6.1); при расхождении CI выдаст точное место.

## QA-скрипт (ручная проверка)

1. Library → перетащить `.mrpack`: оверлей «Drop to import» → прогресс копирования → диалог «Modrinth pack» с числом модов → Create → сборка появилась во вкладке «yours».
2. Перетащить CurseForge zip и Prism zip: соответствующий формат в диалоге; для частичного пака (без config/kubejs) — жёлтый блок «Missing folders» и успешное создание.
3. Перетащить zip, где всё обёрнуто в папку `MyPack/` — формат определяется верно.
4. Перетащить папку `mods` (или zip с `mods/`): content-импорт, моды в сборке.
5. Перетащить папку `resourcepacks` (или `shaders`) — сборка создаётся с этими ресурсами, без ошибок «not an instance».
6. Перетащить `.rar` / `.7z` версии любого из п.1–5 — работает через конвертацию; зашифрованный 7z даёт понятную ошибку.
7. Во время копирования нажать Escape/убрать курсор — прогресс не срывается; отмена перед созданием удаляет стейдж.
8. Темы: оверлей и диалог на glass/sharp темах — читаемы (используют токены).

## Бэклог

- Потоковая загрузка без буферизации файла целиком в памяти (`arrayBuffer`) — сейчас до ~размера файла ОЗУ на время чтения.
- Прогресс пофайлово (имя текущего файла) вместо суммарного процента.
- Поддержка многотомных .rar (part1/part2) — unrar умеет `open_for_listing_split`.
- Drag-импорт в другие вкладки (Discover-карточки → «Добавить в сборку»).
