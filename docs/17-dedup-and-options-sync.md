# 17. План: дедупликация файлов и синхронизация options.txt

Дата: 2026-09-04. Источник идеи: `deduplication-help.txt` (pnpm-подобный
Content-Addressable Storage на hardlinks).

> **СТАТУС (2026-09-04): РЕАЛИЗОВАНО.** Все четыре milestone выполнены:
> M1 options.txt sync (1defe7a), M2 dedup Mojang-загрузок (881800f),
> M3 ретро-дедуп + честный GC (f9c880b, 03c3f6c), M4 Storage UI (57fc512),
> плюс hardening store (уникальные tmp, retry hardlink, кэш валидации
> lookup). Разделы ниже описывают план как он задумывался; фактические
> расхождения от плана помечены «Факт:».

---

## Часть 1. Дедупликация файлов сборок

### 1.1 Что уже есть (не строить заново)

`crates/tuffbox-core/src/mod_store.rs` — уже реализован pnpm-паттерн:

- Store root: `%LOCALAPPDATA%/TuffBox/modstore/objects/<xx>/<sha1>`.
- `try_hardlink(target, sha1)` — линкует объект в инстанс без скачивания.
- `record(file, sha1)` — атомарно (tmp + rename) складывает свежескачанный
  файл в store; перед записью проверяет реальный хеш.
- `gc()` — удаляет объекты, на которые нет ссылок (Unix: `nlink == 1`;
  Windows: консервативно по age > 30 дней).
- Путь вызова: `mod_files::materialize_mod_file_with_progress` — перед
  скачиванием пробует hardlink, после скачивания записывает в store.
- Fallback: любая ошибка store — не фатальна, идёт обычное скачивание
  (dedup — оптимизация, не требование).

Это покрывает **только моды** (`mods/*.jar`). Расширяем.

### 1.2 Что дедуплицировать (по приоритету)

| Категория | Папки | Выгода | Риск |
|-----------|-------|--------|------|
| Библиотеки Java / loader | `libraries/` из манифестов Mojang | Огромная: у 10 сборок на одну версию библиотеки идентичны | Нет — файлы неизменяемы |
| Assets Minecraft | `assets/` (звуки, текстуры ванилы) | Огромная: сотни МБ на версию | Нет |
| Ресурспаки | `resourcepacks/*.zip` | Высокая: игроки копируют паки между сборками | Низкий — zip обычно не редактируют |
| Шейдеры | `shaderpacks/*.zip` | Высокая — то же | Низкий |
| Моды | `mods/*.jar` | Уже сделано | — |
| Конфиги, saves, options.txt | `config/`, `saves/`, `options.txt` | — | **ЗАПРЕЩЕНО линковать** (мутируемые) |

Важно: не трогаем `config/`, `saves/`, `screenshots/`, `options.txt` —
hardlink сделает изменение в одной сборке видимым во всех (общий inode).

### 1.3 Архитектура: единый TuffStore вместо трёх

Переименование не обязательно; расширяем `mod_store` в общий модуль
`tuffbox-core/src/tuff_store.rs` (или переиспользуем mod_store с новыми
публичными функциями). Структура:

```
%LOCALAPPDATA%/TuffBox/store/
├── objects/<xx>/<sha1>          # все дедуплицируемые файлы
└── meta/<sha1>.json             # опционально: происхождение, readonly-флаг
```

Ключевое правило **одного тома**: store и инстансы должны быть на одном
диске. TuffBox разрешает произвольные папки проектов → проверяем том:
- Windows: сравнение префикса диска (`U:\` vs `C:\`) или `fs::hard_link`
  возвращает `CrossesDevices` → fallback на копирование (уже реализовано
  как «ошибка = нет хита»).
- Если проект на другом диске — store создаётся второй, локальный:
  `<project>/.tuffstore/objects/...` (тот же код, другой root).

### 1.4 Изменения по файлам

1. **`mod_files.rs`** — уже работает. Добавить материализацию
   resourcepacks/shaderpacks через тот же путь: у Modrinth-проектов типа
   `resourcepack`/`shader` тоже есть sha1 → `try_hardlink`/`record`
   работают без изменений кода. Нужно только пропустить их через
   `materialize_*` (сейчас, вероятно, отдельный путь установки).
2. **`mc_install.rs` (библиотеки)** — при скачивании библиотек из
   version manifest (есть `sha1` в манифесте Mojang) — обернуть тем же
   паттерном: `try_hardlink` → download → `record`. Наибольший выигрыш
   на одного игрока.
3. **`mc_install.rs` (assets)** — объекты < 1 МБ, их тысячи; hardlink на
   каждый файл ок, но выигрыш только между сборками на одной версии.
   Делать после библиотек.
4. **Ретроспективная дедупликация** — команда «Deduplicate existing packs»:
   - скан всех проектов TuffBox (список папок из реестра проектов);
   - для каждого `mods/*.jar`, `resourcepacks/*.zip`, `shaderpacks/*.zip`,
     `libraries/**` — посчитать sha1 (кэшировать в `mod_index_cache`);
   - если объект в store — заменить файл на hardlink
     (`remove_file` + `hard_link`, атомарно через tmp-name);
   - если нет — `record` его в store и линковать остальные копии.
   - UI: страница Maintenance/Diagnostics, прогресс + «освобождено N ГБ»
     (считать как `sum(size) - size_of(уникальных)`).
5. **GC** — уже есть; для Windows заменить age-эвристику: вести реестр
   ссылок (`meta/<sha1>.json` — список проектов) или расширить скан.
   Ближайший честный вариант: при ретро-скане/установке проверять
   `nlink` через `std::os::windows::fs::MetadataExt::number_of_links()`
   (стабильно с Rust 1.68+) → убрать age-эвристику целиком.
6. **Readonly-защита** — при `record()` ставить объекту read-only
   (`PermissionsExt` на Unix, `readonly(true)` на Windows). Игрок не
   испортит jar, который расшарен между 10 сборками.
7. **Проверка целостности** — `lookup()` уже перехеширует объект; для
   больших libraries это дорого → кэшировать результат проверки с TTL
   через `api_cache::get_or_insert_with_ttl` (паттерн уже в проекте).

### 1.5 Не делать (осознанно)

- Reflink/copy-on-write (APFS clonefile, ReFS) — вне охвата, NTFS-большинство.
- Дедупликация `saves/`, миров, `config/` — опасно и бесполезно.
- Symlink-режим — не нужен, hardlink + copy fallback достаточно.

---

## Часть 2. Синхронизация options.txt по группам версий MC

### 2.1 Требования пользователя

- Один общий `options.txt`, разделяемый между сборками.
- Группировка по **версиям Minecraft** (настройки несовместимы между
  мажорами: `1.21.x` vs `1.20.x` vs `1.12.2`).
- Возможность **переопределения** для конкретной сборки.
- **Никогда не перезаписывать** существующий options.txt инстанса
  (защита от потери настроек игрока).

### 2.2 Модель

```
%LOCALAPPDATA%/TuffBox/
└── options-profiles/
    ├── <group-id>/options.txt          # разделяемый шаблон группы
    └── registry.json                   # { "group-id": { "mc_versions": [...], "name": "..." } }
```

**Группы версий** — автоматические по data-epoch (уже есть
`ModpackEnvironment::data_epoch()` в `environment.rs` — переиспользуем!):

| Группа | Версии |
|--------|--------|
| `modern-1.21+` | 1.21, 1.21.x, 26.x (новые) |
| `1.20.x` | 1.20 – 1.20.6 |
| `1.19.x` | 1.19 – 1.19.4 |
| `1.18.x` | 1.18 – 1.18.2 |
| `1.17.x` | 1.17 – 1.17.1 |
| `1.16.x` | 1.16 – 1.16.5 |
| `legacy-1.12` | 1.13 и старше |

Правило: группа = по мажорной версии до первого несовместимого изменения
формата options.txt. При незнакомой версии — отдельная группа `mc-<version>`.

### 2.3 Алгоритм (строго без перезаписи)

Момент применения: `launcher::prepare_instance` (до запуска игры).

```
fn sync_options(instance_dir, mc_version):
    shared  = options_profiles/<group(mc_version)>/options.txt
    local   = instance_dir/options.txt
    stamp   = instance_dir/.tuffbox-options-managed   # маркер управления

    if local.exists():
        if !stamp.exists():
            return   # пользовательский файл — НИКОГДА не трогаем
        # файл под нашим управлением: сравнить содержимое
        if sha(local) == sha(shared): return
        # shared изменился → обновить локальный АТОМАРНО:
        #   1. бэкап: copy local -> instance_dir/options.txt.bak-<ts>
        #   2. copy shared -> tmp; rename tmp -> local
        #   (объединение ключей — см. 2.4)
    else:
        if shared.exists():
            copy shared -> local; create stamp
        else:
            return   # нечего синхронизировать; игра создаст свой
```

Когда **игрок меняет настройки в игре** — Minecraft перезаписывает
`local options.txt`. После выхода из игры (или при следующем запуске
TuffBox) — **записываем обратно**: copy local -> shared, если local
под управлением (stamp существует). Так общий файл «учится» у игрока.

### 2.4 Merge-стратегия при конфликте (local vs shared)

Ключ: `key=value` (парсер уже есть — `properties_parser.rs`).

Приоритет источника при обновлении shared → local:
- **Берём local** для всех ключей, которые игрок менял после последнего
  sync (хранить snapshot `last-synced.txt` рядом со stamp);
- **Берём shared** для ключей, которых нет в local (новые настройки
  пришли из другой сборки той же группы).

Реализация: три файла — shared, local, last-synced (значения на момент
прошлого sync). 3-way merge:
- local == last_synced → ключ не менялся игроком → взять shared;
- local != last_synced → игрок менял → оставить local.

### 2.5 Override для конкретной сборки

- `instance_dir/.tuffbox-options-managed` → отсутствие файла = сборка
  полностью независима (пользовательский режим).
- UI: в настройках сборки переключатель
  «Use shared options (group 1.21.x) / Independent options».
  - «Independent» = удалить stamp; локальный файл остаётся как есть.
  - Включение обратно = предложение импортировать текущий local в
    группу или принять групповой (диалог, не молча).
- Кнопка «Push current options to group» — скопировать local -> shared.

### 2.6 Удаление / миграции

- Удаление сборки никогда не удаляет shared-файл.
- При изменении версии MC сборки (обновление сборки на 1.22) — группа
  пересчитывается; stamp сохраняется, но первый sync новой группы
  делает merge (не overwrite) по правилам 2.4.
- `options.txt.bak-<ts>` — хранить последние 3, старше удалять.

### 2.7 Изменения по файлам

| Файл | Изменение |
|------|-----------|
| `crates/tuffbox-core/src/options_sync.rs` (новый) | группы версий, sync/merge/push, stamp-логика |
| `crates/tuffbox-core/src/launcher.rs::prepare_instance` | вызов `sync_options` перед запуском |
| `crates/tuffbox-core/src/launcher.rs` (после выхода процесса) | «write-back» local → shared |
| `apps/tuffbox-desktop/src-tauri/src/lib.rs` | Tauri-команды: get/set mode, push-to-group, list groups |
| `apps/tuffbox-desktop/src/components/...` | UI-переключатель режима + список групп |
| `crates/tuffbox-core/src/properties_parser.rs` | переиспользуется для 3-way merge |

### 2.8 Тесты

- `sync_creates_local_from_shared` — новая сборка получает групповой файл.
- `never_overwrites_user_file` — нет stamp → файл не тронут даже при
  разном содержимом.
- `backup_created_on_managed_update`.
- `three_way_merge_keeps_player_edits` — игрок менял `gamma`, shared
  менял `renderDistance` → в результате оба изменения.
- `override_mode_is_independent` — без stamp локальные правки не улетают
  в shared.
- `write_back_after_launch` — правки из игры попадают в shared.

---

## Часть 3. Порядок внедрения (выполнено)

1. **M1 — options.txt sync** ✔ (1defe7a): группы `mc-1.<minor>` (не data_epoch —
   Факт: options.txt несовместим между минорными версиями, гранулярность —
   minor), stamp `.tuffbox-options-managed`, 3-way merge, write-back,
   5 Tauri-команд, секция «Shared options.txt» в Project Settings.
2. **M2 — расширение dedup** ✔ (881800f): Факт: реализовано одной точкой —
   `mc_install::download_with_sha1` (client jar, libraries, natives, assets
   Mojang все идут через неё). Ресурспаки/шейдеры уже проходили через
   `materialize_mod_file` с store.
3. **M3 — ретро-дедупликация + GC** ✔ (f9c880b, 03c3f6c):
   `mod_store::retro_dedup` + `store_retro_dedup` Tauri-команда (свип
   recent projects в spawn_blocking). GC: Факт: `number_of_links()` в std
   нестабилен — Windows использует file identity через `same-file`
   (volume serial + file index) для точной проверки «уже линк», GC держит
   grace 24h для свежих объектов (Unix: nlink). Readonly-защита объектов
   store не сделана (объекты перезаписываются только идентичным контентом,
   риск ниже, чем стоимость); отложено.
4. **M4 — Storage UI** ✔ (57fc512): Settings → Launcher → Storage:
   статистика store, «Deduplicate existing projects», «Clean unused store
   files».

Hardening store (после аудита):
- `record()`: уникальное tmp-имя (pid + counter + thread id) — раньше
  два параллельных луча rayon могли гоняться за одним `.tmp-<pid>`.
- `try_hardlink()`: retry через 150 мс при ERROR_SHARING_VIOLATION
  (антивирус/индексатор на Windows).
- `lookup()`: кэш валидации по (size, mtime) fingerprint — без него
  каждый Play перехешировал все объекты store (стоп на больших library jar).

Риски и границы (актуально):
- cross-device → fallback copy (уже в коде), локальный store на другой том.
- антивирусы/индексаторы Windows иногда держат файл открытым →
  ретрай с задержкой, затем fallback copy.
- нелинковать ничего в `saves/`, `config/`, `screenshots/` — соблюдаются
  и в retro_candidates, и в M2-путях.

---

## Часть 4. Аудит безопасности + per-pack opt-out (2026-09-16)

> **СТАТУС: РЕАЛИЗОВАНО.** Полный аудит механизма + исправления найденных
> уязвимостей + выключение дедупликации для отдельного модпака.

### 4.1 Аудит «теряются ли файлы пользователя»

Классическая семантика hard links: удаление одного имени не трогает данные,
пока жив другой. Проверенные пути:

| Операция | Вердикт |
|---|---|
| Удаление инстанса/мода | safe: снимается одна ссылка, объект store жив |
| Экспорт (.mrpack/server) | safe: только чтение (read_export_file, ретраи) |
| Замена мода при обновлении | safe: `.part` + rename — атомарная замена записи каталога |
| Включение/выключение мода (.disabled) | safe: rename, тот же inode |
| Запуск/верификация | safe: только чтение/хеширование |
| GC store | safe по nlink/identity + grace 24h (см. 4.3) |

Найденные и исправленные уязвимости (все — вариант «запись поверх
существующего файла», мутирующая общий inode):

1. **snapshot restore** (`snapshot.rs::copy_file`) и восстановление файла
   снапшота (`lib.rs`) писали `fs::copy` «в место» — при восстановлении
   поверх hardlink'а в mods/ байты улетали бы в общий объект store
   (порча всех сборок сразу). → `fs_util::copy_replacing`
   (tmp + атомарный rename, старый inode только отвязывается).
2. **`helpers::copy_dir_recursive`** и **github- pack `copy_tree`** — тот же
   in-place `fs::copy` при переустановке поверх существующего пака.
   → переведены на `copy_replacing`.
3. **`fs_util::restore_from_backup`** — тот же паттерн для конфигов
   (не линкуются, но защита «на будущее»). → `copy_replacing`.
4. **`mc_install::download_with_sha1`** — удаление несовпадающего файла
   перед перекачкой падало бы на read-only линке (Windows). →
   `fs_util::clear_readonly` перед remove.
5. **`relink_to_store`** имел окно remove→link (краш = потерянный файл). →
   линк создаётся под уникальным tmp-именем и атомарно переименовывается;
   при любой ошибке исходный файл остаётся на месте (не нужен copy-restore).

Регресс-тесты: `restore_replaces_hardlinks_without_corrupting_siblings`,
`copy_replacing_never_mutates_a_hardlinked_destination`,
`restore_from_backup_keeps_hardlinked_siblings_intact` — все падают на
старом in-place коде и проходят на новом.

### 4.2 Per-pack opt-out (запрос пользователя)

Маркер `.tuffbox-no-dedup` в корне проекта:

- **materialize_mod_file** (установка/синк модов): при маркере store не
  консультируется и не пополняется — пак получает независимые копии.
- **retro_dedup**: корень с маркером пропускается целиком
  (`RetroReport.disabled_roots`).
- **UI**: Project Settings → «File deduplication» — статус-бейдж
  (Shared store / Independent files), переключение в любой момент:
  - off → маркер + `materialize_project`: каждый jar/zip, являющийся
    ссылкой в store, заменяется независимой копией тех же байтов
    (tmp + атомарный rename; краш в худшем случае оставляет файл
    линком, потеря данных невозможна). Store и другие паки не затронуты.
  - on → маркер снимается + одиночный retro-dedup этого корня.
- Команды: `dedup_project_status`, `dedup_project_set_enabled`
  (spawn_blocking — хеширование уходит с главного потока).
- Тесты: `retro_dedup_skips_packs_that_opted_out`,
  `materialize_project_restores_independence`,
  `materialize_mod_file_respects_per_pack_opt_out`.

### 4.3 GC: единая grace-политика

Unix удалял свежезаписанные объекты (nlink==1) немедленно — кэш store
стирался сразу после первой установки, до того как второй инстанс успеет
линкнуть. Теперь 24-часовой grace применяется на всех ОС (документированная
политика). Тест: `fresh_unlinked_object_survives_gc`.

### 4.4 Импорт сборок (Prism и др.) + вопрос о дедупе (2026-09-16, доп.)

Ответ на вопрос «импортирую 2 сборки Prism — они дедуплицируются?»:
- **zip-импорт** (Prism zip / .mrpack / CF): моды качаются через
  `materialize_mod_file` → store используется автоматически ✓.
- **folder-импорт** (папка Prism): контент копируется как есть → до сих
  пор НЕ дедуплицировался между сборками.

Теперь **при каждом импорте спрашивается решение** (docs/17 §4):
- AddInstanceModal (страница Import): чипы «Share identical files
  (recommended) / Keep independent files», по умолчанию share.
- Library / LibraryInstancesPane (файл, папка инстанса, drop-импорт):
  диалог `DedupAskDialog` ДО запуска импорта (GitHub-импорт не спрашивает
  — там свой флоу). Закрытие диалога = отмена импорта.
- `install_modpack` принимает `dedup: Option<bool>`:
  - `Some(true)`  → после установки `retro_dedup` одного корня: второй
    Prism-пак сразу линкуется к байтам первого (в ответе `dedup.linked` /
    `bytesReclaimed`, UI показывает тост «N file(s) shared, ~X MB saved»);
  - `Some(false)` → маркер `.tuffbox-no-dedup`;
  - `None`        → прежнее поведение (совместимость, CreationTrends).

### 4.4a Аудит всех pack-импортов (2026-09-16, доп. 2)

Проверены ВСЕ пути, создающие новый пак (не только Prism):

| Путь | Вопрос о дедупе |
|---|---|
| AddInstanceModal (файл .mrpack/zip) | чипы на странице импорта ✓ |
| Library / LibraryInstancesPane: файл, папка, drop | DedupAskDialog ✓ |
| Library → Discover «Add to library» (api.modpacks.install) | DedupAskDialog ✓ (добавлено) |
| CreationTrends (уставка модпака из трендов) | DedupAskDialog ✓ (добавлено) |
| GitHub-пак (вкл. deep links tuffbox://install) | DedupAskDialog ✓ (добавлено; раньше делегировал в github_pack_install мимо dedup — теперь `dedup` пробрасывается и применяется к `final_instance_dir`) |
| Шаблоны (save_as_template) | не нужен — только метаданные |
| Optimize-паки / пресеты / одиночные моды | не нужен — ставятся в существующий пак, подчиняются его маркеру |
| `import_project` / `import_curseforge_project` (legacy-команды) | UI не использует; поведение по умолчанию |

### 4.5 Полное удаление файла (2026-09-16, доп.)

«Пользователь удалил файл, других пользователей нет → место на диске
должно освободиться»: `mod_store::release(sha1, instance_roots)` удаляет
объект store, когда его больше никто не линкует:
- Unix: точно по nlink (после снятия ссылки пака nlink==1 → удалить);
- Windows: identity-скан известных корней проектов (same-file). Неизвестные
  папки безопасны: их ссылки держат inode, store лишь теряет кэш-имя.
- Без grace-периода (это явное удаление, не эвристика); гонки с параллельным
  record/link деградируют в «кэш-мисс → перекачка», не в потерю данных.

Врезано в пути удаления (через `release_store_object_detached` —
отдельный поток, не блокирует UI): remove_loose_jar,
keep_one_duplicate_mod_jar, remove_mod_file_from_disk (хеш из манифеста
или перехеш перед удалением), same-bytes cleanup. Тест:
`release_frees_space_only_when_last_linker_is_gone`.

### 4.6 Идеи развития (не реализовано, backlog)

- Reflink/copy-on-write (ReFS/APFS/Btrfs `clonefile`) — нулевая цена копии
  там, где поддерживается.
- Windows: честный nlink через `number_of_links()` при стабилизации в std
  (или winapi) — уберёт age-эвристику в GC.
- `meta/<sha1>.json` реестр происхождения объектов (откуда скачан, счётчик
  линков) для диагностики и точного GC без эвристик.
- Дедуп `libraries/` инстанс-локальных лоадеров (сейчас только shared
  runtime) и `assets/` между версиями.
- Фоновый ретро-дедуп новых проектов после первой установки (сейчас —
  только вручную из Settings → Storage).
- Пер-пак opt-out для shared runtime libraries (сейчас флаг действует на
  файлы внутри папки пака).
