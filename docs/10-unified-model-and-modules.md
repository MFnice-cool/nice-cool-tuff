# 10. Unified Model Layer и расширение модулей

Этот документ описывает архитектурное расширение TuffBox поверх существующего ядра. Всё новое добавляется в `tuffbox-core` как Rust-модули. UI (Svelte) не меняется.

## Текущая структура (база)

```text
crates/tuffbox-core/src/
  manifest.rs, graph.rs, diagnostics.rs, crash.rs, change_plan.rs,
  lockfile.rs, resolver.rs, snapshot.rs, exporter.rs, importer.rs,
  launcher.rs, mc_install.rs, forge.rs, jre.rs, process.rs,
  http.rs, versions.rs, provider/
```

Все типы, описанные ниже, добавляются как новые модули в `tuffbox-core`.

---

## Архитектурное расширение

```text
┌─────────────────────────────────────────────────────────────────┐
│                        Svelte + TypeScript UI                    │
│              Работает только с унифицированными типами           │
└────────────────────────────┬────────────────────────────────────┘
                             │ IPC (команды Tauri)
┌────────────────────────────▼────────────────────────────────────┐
│                     Unified Model Layer                          │
│                                                                  │
│  UnifiedMod         — метаданные мода                            │
│  UnifiedRecipe      — рецепт из любого загрузчика                │
│  UnifiedTag         — тег предмета из любого загрузчика          │
│  ConfigTree         — дерево конфига из любого формата            │
│  QuestBook          — квесты FTB Quests / HQM / Heracles         │
│  DuplicateGroup     — группа дублирующихся ресурсов              │
│  ModpackProject     — проект модпака со всеми настройками        │
└────────────────────────────┬────────────────────────────────────┘
                             │
┌────────────────────────────▼────────────────────────────────────┐
│                    Loader Adapters Layer                          │
│                                                                  │
│  ForgeAdapter       — мета из mods.toml / mcmod.info             │
│  FabricAdapter      — мета из fabric.mod.json                    │
│  NeoForgeAdapter    — мета из neoforge.mods.toml                 │
│  QuiltAdapter       — мета из quilt.mod.json                     │
│                                                                  │
│  Каждый адаптер знает:                                           │
│  • где лежат рецепты внутри JAR (recipes/ vs recipe/)            │
│  • где лежат теги (tags/items/ vs tags/item/)                    │
│  • как парсить условия (forge:conditional vs neoforge:conditions) │
│  • формат результата рецепта (item vs id, NBT vs Components)     │
└────────────────────────────┬────────────────────────────────────┘
                             │
┌────────────────────────────▼────────────────────────────────────┐
│                   Version Strategy Layer                         │
│                                                                  │
│  DataEpoch::Legacy         (1.0–1.12)  — рецепты в Java, .cfg    │
│  DataEpoch::EarlyDataPack  (1.13–1.15) — первые data packs       │
│  DataEpoch::ModernDataPack (1.16–1.20) — стабильные data packs   │
│  DataEpoch::Components     (1.21+)     — компоненты вместо NBT   │
│                                                                  │
│  TagNamespace::Forge   — forge:ingots/copper                     │
│  TagNamespace::Common  — c:ingots/copper                         │
│  TagNamespace::Mixed   — оба варианта                            │
│                                                                  │
│  RecipeParser для каждой комбинации версии и типа рецепта         │
│  ShapedRecipeParser, ShapedRecipeParser121,                      │
│  GenericRecipeParser (фоллбэк для модовых рецептов)              │
└────────────────────────────┬────────────────────────────────────┘
                             │
┌────────────────────────────▼────────────────────────────────────┐
│                 Три уровня знаний о модах                        │
│                                                                  │
│  Уровень 1: JSON база знаний (mod_knowledge.json)                │
│    Краудсорсинг, поставляется с программой, обновляется через     │
│    GitHub. Содержит проверенные людьми данные: пути конфигов,     │
│    ключи генерации руд, программные предметы, конфликты.         │
│    Покрывает ~15 самых популярных модов.                          │
│                                                                  │
│  Уровень 2: Автоматические эвристики                             │
│    Работает для ~80% модов без каких-либо данных:                 │
│    • Определение владельца конфига по имени файла                 │
│    • Поиск настроек генерации руд по паттернам ключей             │
│      (shouldGenerate, veinSize, minHeight...)                     │
│    • Классификация предметов по паттернам имён                    │
│      (tin_ingot, ingot_tin → ("tin", Ingot))                     │
│    • Нормализация тегов между forge: и c:                        │
│                                                                  │
│  Уровень 3: Фоллбэк на пользователя / ИИ                        │
│    Если эвристики дали низкую уверенность — показываем            │
│    пользователю с пометкой. Пользователь подтверждает или         │
│    исправляет, и его выбор сохраняется в локальную базу знаний.  │
│    ИИ может помочь разобрать незнакомый формат конфига.           │
└────────────────────────────┬────────────────────────────────────┘
                             │
┌────────────────────────────▼────────────────────────────────────┐
│                      Raw File Parsers                            │
│                                                                  │
│  TomlParser       — config/*.toml (Forge/NeoForge)               │
│  JsonParser       — config/*.json (Fabric, data packs)           │
│  Json5Parser      — config/*.json5 (некоторые Fabric моды)       │
│  ForgeCfgParser   — config/*.cfg (Legacy Forge 1.12 и ранее)    │
│  SnbtParser       — FTB Quests, FTB Chunks и другие FTB моды    │
│  PropertiesParser — server.properties, некоторые моды            │
│                                                                  │
│  Все парсеры реализуют трейт ConfigParser:                       │
│    fn parse(&self, content, path) → ConfigFile                   │
│    fn serialize(&self, config) → String                          │
│    fn can_handle(&self, path, content) → bool                    │
│                                                                  │
│  Все форматы парсятся в единое дерево ConfigTree:                │
│    ConfigValue::Bool / Integer / Float / String / List / Table   │
│    ConfigEntry = value + default + comment + range + allowed     │
└─────────────────────────────────────────────────────────────────┘
```

---

## Структура новых Rust-модулей

Все новые модули добавляются в `crates/tuffbox-core/src/`:

```text
crates/tuffbox-core/src/
  ├── unified/              ← Unified Model Layer
  │   ├── mod.rs
  │   ├── mod_entity.rs     — UnifiedMod
  │   ├── recipe.rs         — UnifiedRecipe
  │   ├── tag.rs            — UnifiedTag
  │   ├── config_tree.rs    — ConfigTree + ConfigValue + ConfigEntry
  │   ├── quest_book.rs     — QuestBook, Chapter, Quest, Task, Reward
  │   ├── duplicate.rs      — DuplicateGroup
  │   └── project.rs        — ModpackProject
  │
  ├── adapters/             ← Loader Adapters Layer
  │   ├── mod.rs            — LoaderAdapter trait
  │   ├── forge.rs          — ForgeAdapter
  │   ├── fabric.rs         — FabricAdapter
  │   ├── neoforge.rs       — NeoForgeAdapter
  │   └── quilt.rs          — QuiltAdapter
  │
  ├── parsers/              ← Raw File Parsers
  │   ├── mod.rs            — ConfigParser trait
  │   ├── toml.rs           — TomlParser
  │   ├── json.rs           — JsonParser
  │   ├── json5.rs          — Json5Parser
  │   ├── forge_cfg.rs      — ForgeCfgParser
  │   ├── snbt.rs           — SnbtParser
  │   └── properties.rs     — PropertiesParser
  │
  ├── knowledge/            ← Три уровня знаний
  │   ├── mod.rs
  │   ├── builtin.rs        — Загрузка mod_knowledge.json
  │   ├── heuristics.rs     — Автоэвристики
  │   └── user.rs           — Локальная база пользователя
  │
  └── strategy/             ← Version Strategy Layer
      ├── mod.rs
      ├── epoch.rs          — DataEpoch
      ├── namespace.rs      — TagNamespace
      └── recipe_parser.rs  — RecipeParser, ShapedRecipeParser, GenericRecipeParser
```

---

## Unified Model Layer

### UnifiedMod

```rust
pub struct UnifiedMod {
    pub id: String,
    pub slug: String,
    pub name: String,
    pub version: String,
    pub mc_versions: Vec<String>,
    pub loader: LoaderKind,
    pub side: Side,
    pub source: ModSource,
    pub dependencies: Vec<ModDependency>,
    pub provides: Vec<String>,           // предметы, которые мод добавляет
    pub config_files: Vec<String>,       // пути к конфигам
    pub tags: Vec<String>,               // пользовательские теги
}
```

### UnifiedRecipe

```rust
pub struct UnifiedRecipe {
    pub id: String,
    pub recipe_type: RecipeType,         // shaped, shapeless, smithing, stonecutting, smelting, blasting, smoking, campfire
    pub ingredients: Vec<UnifiedIngredient>,
    pub result: UnifiedItem,
    pub namespace: String,               // namespace мода
    pub loader: LoaderKind,
    pub conditions: Vec<RecipeCondition>, // forge:conditional, neoforge:conditions
}

pub struct UnifiedIngredient {
    pub item: String,                    // item id
    pub tag: Option<String>,             // tag reference
    pub count: u32,
    pub nbt: Option<serde_json::Value>,  // Legacy NBT
    pub components: Option<serde_json::Value>, // 1.21+ Components
}

pub struct UnifiedItem {
    pub item: String,
    pub count: u32,
    pub nbt: Option<serde_json::Value>,
    pub components: Option<serde_json::Value>,
}

pub enum RecipeType {
    Shaped,
    Shapeless,
    Smithing,
    Stonecutting,
    Smelting,
    Blasting,
    Smoking,
    Campfire,
    Custom(String),
}
```

### UnifiedTag

```rust
pub struct UnifiedTag {
    pub namespace: TagNamespace,
    pub path: String,                    // e.g. "ingots/copper"
    pub values: Vec<String>,             // item ids
    pub source_mods: Vec<String>,        // какие моды добавляют в тег
}

pub enum TagNamespace {
    Forge,    // forge:ingots/copper
    Common,   // c:ingots/copper
    Fabric,   // fabric:ingots/copper
    Mixed,    // несколько пространств имён
    Unknown,
}
```

### ConfigTree

```rust
pub struct ConfigTree {
    pub entries: Vec<ConfigEntry>,
    pub path: String,
    pub format: ConfigFormat,
    pub owner_mod: Option<String>,
}

pub struct ConfigEntry {
    pub key: String,
    pub value: ConfigValue,
    pub default: Option<ConfigValue>,
    pub comment: Option<String>,
    pub range: Option<ConfigRange>,
    pub allowed: Option<Vec<String>>,
    pub changed: bool,
}

pub enum ConfigValue {
    Bool(bool),
    Integer(i64),
    Float(f64),
    String(String),
    List(Vec<ConfigValue>),
    Table(BTreeMap<String, ConfigValue>),
}

pub struct ConfigRange {
    pub min: f64,
    pub max: f64,
}

pub enum ConfigFormat {
    Toml,
    Json,
    Json5,
    ForgeCfg,
    Properties,
}
```

### QuestBook

```rust
pub struct QuestBook {
    pub chapters: Vec<Chapter>,
}

pub struct Chapter {
    pub id: String,
    pub title: String,
    pub quests: Vec<Quest>,
}

pub struct Quest {
    pub id: String,                      // 16-char hex
    pub title: String,
    pub description: String,
    pub icon: Option<String>,            // item id
    pub dependencies: Vec<String>,       // quest ids
    pub tasks: Vec<Task>,
    pub rewards: Vec<Reward>,
    pub position: Option<QuestPosition>,
}

pub struct QuestPosition {
    pub x: f64,
    pub y: f64,
}

pub enum TaskType {
    Item { item: String, count: u32 },
    Kill { entity: String, count: u32 },
    Dimension { dimension: String },
    Biome { biome: String },
    Stat { stat: String, value: u64 },
    Checkmark,
    GameStage { stage: String },
    Custom { task_type: String, data: serde_json::Value },
}

pub enum RewardType {
    Item { item: String, count: u32 },
    Xp { amount: u32 },
    XpLevels { levels: u32 },
    Command { command: String },
    RandomReward { table: Vec<String> },
    Choice { options: Vec<String> },
    Custom { reward_type: String, data: serde_json::Value },
}
```

### DuplicateGroup

```rust
pub struct DuplicateGroup {
    pub material: String,                // "tin", "copper", "iron"
    pub item_type: String,               // "ingot", "ore", "dust", "plate"
    pub variants: Vec<DuplicateVariant>,
    pub recommended: Option<String>,     // item id основного варианта
    pub confidence: Confidence,
}

pub struct DuplicateVariant {
    pub item_id: String,
    pub mod_id: String,
    pub recipe_count: u32,
    pub has_ore_generation: bool,
    pub popularity_score: f64,
}

pub enum Confidence {
    High,
    Medium,
    Low,
}
```

### ModpackProject

```rust
pub struct ModpackProject {
    pub manifest: ProjectManifest,
    pub mods: Vec<UnifiedMod>,
    pub recipes: Vec<UnifiedRecipe>,
    pub tags: Vec<UnifiedTag>,
    pub configs: Vec<ConfigTree>,
    pub quest_book: Option<QuestBook>,
    pub duplicates: Vec<DuplicateGroup>,
    pub knowledge: ModKnowledge,
}
```

---

## Loader Adapters Layer

### LoaderAdapter trait

```rust
pub trait LoaderAdapter {
    /// Читает unified metadata из JAR-файла
    fn extract_metadata(&self, jar_path: &Path) -> Result<UnifiedMod>;

    /// Возвращает пути к рецептам внутри JAR
    fn recipe_paths(&self) -> Vec<&str>;

    /// Возвращает пути к тегам внутри JAR
    fn tag_paths(&self) -> Vec<&str>;

    /// Парсит рецепт из JSON
    fn parse_recipe(&self, json: &serde_json::Value, epoch: DataEpoch) -> Result<UnifiedRecipe>;

    /// Парсит тег из JSON
    fn parse_tag(&self, json: &serde_json::Value) -> Result<UnifiedTag>;

    /// Проверяет условия (forge:conditional, neoforge:conditions)
    fn check_conditions(&self, conditions: &[serde_json::Value]) -> bool;

    /// Формат результата рецепта (item vs id, NBT vs Components)
    fn format_result(&self, result: &serde_json::Value, epoch: DataEpoch) -> Result<UnifiedItem>;
}
```

### ForgeAdapter

```rust
pub struct ForgeAdapter;

// mods.toml → dependencies, loaderVersion, mcVersion
// mcmod.info → name, description, dependencies (Legacy 1.12)
// recipes/ → data/{namespace}/recipes/*.json
// tags/items/ → data/{namespace}/tags/items/*.json
// conditions: forge:conditional, ForgeRegistries.Keys
// result format: { "item": "modname:copper_ingot", "count": 1 }
```

### FabricAdapter

```rust
pub struct FabricAdapter;

// fabric.mod.json → depends, recommends, suggest, conflicts
// data/{namespace}/recipe/*.json (1.13-1.14) или recipes/*.json (1.15+)
// data/{namespace}/tags/items/*.json
// conditions: fabric:load_conditions
// result format: { "id": "modname:copper_ingot", "count": 1 }
```

### NeoForgeAdapter

```rust
pub struct NeoForgeAdapter;

// neoforge.mods.toml → dependencies, loaderVersion, mcVersion
// data/{namespace}/recipes/*.json
// data/{namespace}/tags/items/*.json
// conditions: neoforge:conditions
// result format: { "id": "modname:copper_ingot", "components": {...} } (1.21+)
```

### QuiltAdapter

```rust
pub struct QuiltAdapter;

// quilt.mod.json → depends, recommends, conflicts
// data/{namespace}/recipes/*.json
// data/{namespace}/tags/items/*.json
// conditions: quilt:load_conditions
// result format: как Fabric
```

---

## Version Strategy Layer

### DataEpoch

```rust
pub enum DataEpoch {
    /// 1.0–1.12: рецепты в Java-коде, .cfg конфиги
    Legacy,
    /// 1.13–1.15: первые data packs, нестабильный формат
    EarlyDataPack,
    /// 1.16–1.20: стабильные data packs, JSON рецепты и теги
    ModernDataPack,
    /// 1.21+: компоненты вместо NBT, обновлённый формат
    Components,
}

impl DataEpoch {
    pub fn from_mc_version(version: &str) -> Self;
    pub fn recipe_format(&self) -> RecipeFormat;
    pub fn supports_nbt(&self) -> bool;
    pub fn supports_components(&self) -> bool;
}
```

### TagNamespace

```rust
pub enum TagNamespace {
    Forge,    // forge:ingots/copper
    Common,   // c:ingots/copper
    Fabric,   // fabric:ingots/copper
    Mixed,
    Unknown,
}

impl TagNamespace {
    pub fn normalize(tag: &str, loader: LoaderKind) -> String;
    pub fn detect(tag: &str) -> Self;
}
```

### RecipeParser

```rust
pub trait RecipeParser {
    fn parse(&self, json: &serde_json::Value, epoch: DataEpoch) -> Result<UnifiedRecipe>;
    fn can_handle(&self, recipe_type: &str, epoch: DataEpoch) -> bool;
}

pub struct ShapedRecipeParser;
pub struct ShapedRecipeParser121;      // 1.21+ с компонентами
pub struct ShapelessRecipeParser;
pub struct SmithingRecipeParser;
pub struct StonecuttingRecipeParser;
pub struct SmeltingRecipeParser;
pub struct GenericRecipeParser;       // фоллбэк для модовых рецептов
```

---

## Три уровня знаний о модах

### Уровень 1: JSON база знаний

Файл `mod_knowledge.json` поставляется с программой, обновляется через GitHub.

```json
{
  "mekanism": {
    "config_paths": ["config/mekanism/", "config/mekanism-generators/"],
    "ore_keys": ["shouldGenerate", "veinSize", "minHeight", "maxHeight"],
    "programmatic_items": ["mekanism:enriched"],
    "known_conflicts": ["thermalexpansion"],
    "popularity_score": 90
  },
  "create": {
    "config_paths": ["config/create/"],
    "ore_keys": [],
    "programmatic_items": [],
    "known_conflicts": [],
    "popularity_score": 100
  }
}
```

### Уровень 2: Автоматические эвристики

Работает для ~80% модов без каких-либо данных:

- **Определение владельца конфига** — по имени файла/папки: `mekanism.cfg` → `mekanism`, `create/` → `create`
- **Поиск настроек генерации руд** — паттерны ключей: `shouldGenerate`, `veinSize`, `minHeight`, `maxHeight`, `frequency`, `weight`
- **Классификация предметов по паттернам имён**:
  - `tin_ingot` → `("tin", Ingot)`
  - `ingot_tin` → `("tin", Ingot)`
  - `raw_tin` → `("tin", RawMaterial)`
  - `tin_ore` → `("tin", Ore)`
  - `tin_dust` → `("tin", Dust)`
  - `tin_plate` → `("tin", Plate)`
  - `tin_gear` → `("tin", Gear)`
  - `tin_nugget` → `("tin", Nugget)`
- **Нормализация тегов** — `forge:ingots/copper` ↔ `c:ingots/copper`

### Уровень 3: Фоллбэк на пользователя / ИИ

- Если эвристики дали низкую уверенность → показываем пользователю с пометкой
- Пользователь подтверждает или исправляет
- Выбор сохраняется в локальную базу знаний
- ИИ может помочь разобрать незнакомый формат конфига

---

## Raw File Parsers

### ConfigParser trait

```rust
pub trait ConfigParser: Send + Sync {
    fn parse(&self, content: &str, path: &str) -> Result<ConfigTree>;
    fn serialize(&self, config: &ConfigTree) -> Result<String>;
    fn can_handle(&self, path: &str, content: &str) -> bool;
}
```

### Реализации

| Парсер | Формат | Где встречается |
|---|---|---|
| `TomlParser` | `.toml` | Forge/NeoForge конфиги,mods.toml |
| `JsonParser` | `.json` | Fabric конфиги, data packs |
| `Json5Parser` | `.json5` | Некоторые Fabric моды |
| `ForgeCfgParser` | `.cfg` | Legacy Forge 1.12 и ранее |
| `SnbtParser` | `.snbt` | FTB Quests, FTB Chunks, FTB Teams |
| `PropertiesParser` | `.properties` | server.properties, некоторые моды |

### ConfigTree (общее дерево)

Все форматы парсятся в единое дерево:

```rust
pub enum ConfigValue {
    Bool(bool),
    Integer(i64),
    Float(f64),
    String(String),
    List(Vec<ConfigValue>),
    Table(BTreeMap<String, ConfigValue>),
}

pub struct ConfigEntry {
    pub key: String,
    pub value: ConfigValue,
    pub default: Option<ConfigValue>,
    pub comment: Option<String>,
    pub range: Option<ConfigRange>,       // из комментариев: Range: 0 ~ 100
    pub allowed: Option<Vec<String>>,     // из комментариев: Allowed: [peaceful, easy, normal, hard]
    pub changed: bool,                     // отличается от дефолта
}
```

---

## Модуль 1: Управление проектом

Расширение текущего `Stage 2` (manifest + lockfile).

### Что добавляется

- **Автоопределение окружения** — сканирование папки и автоопределение загрузчика и версии через:
  - `manifest.json` (CurseForge)
  - `modrinth.index.json` (Modrinth)
  - Содержимое JAR-файлов (через LoaderAdapters)
  - `fabric.mod.json`, `mods.toml`, `neoforge.mods.toml`, `quilt.mod.json`
- **Kanban-доска** — «Нужно сделать / В работе / Тестирование / Готово», хранится в `.tuffbox/kanban.json`
- **Система снимков** — расширение текущего snapshot: кнопка «сохранить снимок», откат к любому
- **Сравнение снимков** — diff: какие моды добавлены/удалены, какие конфиги изменились

### Реализация в Rust

```rust
// crates/tuffbox-core/src/unified/project.rs
impl ModpackProject {
    pub fn detect_from_folder(path: &Path) -> Result<Self>;
    pub fn kanban(&self) -> &KanbanBoard;
    pub fn save_kanban(&mut self, board: KanbanBoard) -> Result<()>;
}
```

---

## Модуль 2: Менеджер модов

Расширение текущего `Stage 4` (provider layer).

### Что добавляется

- **Поиск одновременно по CurseForge API и Modrinth API** — единый результат с указанием источника
- **Фильтры**: версия MC, загрузчик, категория, популярность, дата обновления, лицензия
- **Автоматическое скачивание зависимостей** при установке мода
- **Пакетная установка** — отметить 20 модов и установить разом
- **Визуальный граф зависимостей** (d3-force) — каждый мод — узел, линии — зависимости, красные линии — конфликты
- **При удалении мода** граф показывает, какие моды «осиротеют»
- **Матрица совместимости** — таблица мод×мод с цветовыми индикаторами (🟢🟡🔴)
- **Система тегов** — пользователь присваивает модам теги (core, optional-client, endgame, may-remove) для использования при экспорте

### Реализация в Rust

```rust
// crates/tuffbox-core/src/unified/mod_entity.rs
impl UnifiedMod {
    pub fn dependencies_graph(&self, all_mods: &[UnifiedMod]) -> Vec<GraphEdge>;
    pub fn would_orphan(&self, all_mods: &[UnifiedMod]) -> Vec<String>;
}

// crates/tuffbox-core/src/provider/
pub trait CurseForgeProvider {
    async fn search(&self, query: &str, filters: &SearchFilters) -> Result<Vec<CurseForgeProject>>;
    async fn get_project(&self, project_id: &str) -> Result<CurseForgeProject>;
    async fn get_versions(&self, project_id: &str, mc_version: &str, loader: &str) -> Result<Vec<CurseForgeVersion>>;
}
```

---

## Модуль 3: Редактор конфигов

Расширение текущего `Stage 8` (config editor).

### Что добавляется

- **Сканирование папки config/** — автоопределение формата файла и владельца-мода (через `ConfigParser` + `knowledge`)
- **Визуальные контролы** через `ConfigTree`:
  - `bool` → чекбокс
  - `Integer/Float` → слайдер с диапазоном (из `ConfigEntry.range`)
  - enum → выпадающий список (из `ConfigEntry.allowed`)
  - строка → текстовое поле
- **Подсветка изменённых значений** — `ConfigEntry.changed: true`
- **Глобальный поиск** — «copper» → все упоминания во всех конфигах
- **Сравнение с дефолтом** — сброс отдельного параметра или всего файла
- **Пресеты** — сохранение/загрузка профилей настроек (Easy, Normal, Expert)
- **При обновлении мода** — показ новых параметров, появившихся в новой версии

### Реализация в Rust

```rust
// crates/tuffbox-core/src/parsers/
pub fn scan_config_dir(dir: &Path, knowledge: &ModKnowledge) -> Result<Vec<ConfigTree>>;

// crates/tuffbox-core/src/knowledge/
impl ModKnowledge {
    pub fn detect_owner(&self, file_name: &str, content: &str) -> Option<String>;
}
```

---

## Модуль 4: Унификация ресурсов

Новый модуль. Находит дублирующиеся ресурсы из разных модов и генерирует скрипты для их объединения.

### Сканирование

- Программа открывает каждый JAR как ZIP-архив
- Извлекает рецепты из `data/*/recipes/*.json` или `data/*/recipe/*.json`
- Извлекает теги из `data/*/tags/items/*.json` или `data/*/tags/item/*.json`
- Извлекает список предметов из `assets/*/models/item/*.json`
- Учитывает различия загрузчиков через `LoaderAdapter::recipe_paths()` и `LoaderAdapter::tag_paths()`
- Нормализует теги через `TagNamespace::normalize()`

### Обнаружение дубликатов (два метода)

**По тегам:**
- Если тег `c:ingots/copper` содержит предметы из 3 разных модов → дубликат

**По паттернам имён:**
- `mekanism:ingot_tin`, `thermal:tin_ingot`, `ic2:ingot_tin` → все являются `("tin", Ingot)`
- Классификация по суффиксам/префиксам: `_ingot`, `_ore`, `_dust`, `_plate`, `_gear`, `_nugget`, `raw_`

### Автоматический выбор основного варианта (scoring)

| Фактор | Баллы |
|---|---|
| Ванильный предмет | +10000 |
| Количество рецептов, использующих этот вариант | ×10 |
| Наличие генерации руды | +50 |
| Бонус за популярный мод (Create +100, Mekanism +90, Thermal +85) | +bonus |

Пользователь может переопределить выбор.

### Генерация скриптов (4 целевых формата)

| Формат | Версии | Синтаксис |
|---|---|---|
| KubeJS 6 | 1.19–1.20 | `event.replaceInput()`, `event.replaceOutput()` |
| KubeJS 7 | 1.21+ | Обновлённый синтаксис |
| CraftTweaker | Все | `.replaceIngredient()` |
| Data Pack | Без скриптовых модов | Переопределение тегов через JSON |

### Генерируемые файлы

| Файл | Назначение |
|---|---|
| Серверный скрипт | Замена предметов во всех рецептах |
| Клиентский скрипт | Скрытие дубликатов из JEI/REI/EMI |
| Мировой скрипт | Замена предметов при подборе игроком |
| Data Pack | Переопределение тегов, чтобы указывали только на основной вариант |

### Управление генерацией руд

- Автоматический поиск настроек генерации руд в конфигах через эвристики (`knowledge::heuristics`)
- Для популярных модов — точные данные из JSON базы знаний
- Генерация изменений конфигов: `tin.shouldGenerate = false` для всех модов кроме основного
- Уровень уверенности (High/Medium/Low) для каждого найденного параметра

### Реализация в Rust

```rust
// crates/tuffbox-core/src/unified/duplicate.rs
impl DuplicateGroup {
    pub fn detect_by_tag(tags: &[UnifiedTag]) -> Vec<Self>;
    pub fn detect_by_pattern(mods: &[UnifiedMod]) -> Vec<Self>;
    pub fn score_variants(&mut self, vanilla_items: &[String]);
}

// crates/tuffbox-core/src/unified/project.rs
impl ModpackProject {
    pub fn scan_jars(&mut self) -> Result<()>; // заполняет recipes, tags
    pub fn find_duplicates(&self) -> Vec<DuplicateGroup>;
    pub fn generate_kubejs6_script(&self, duplicates: &[DuplicateGroup]) -> Result<String>;
    pub fn generate_kubejs7_script(&self, duplicates: &[DuplicateGroup]) -> Result<String>;
    pub fn generate_crafttweaker_script(&self, duplicates: &[DuplicateGroup]) -> Result<String>;
    pub fn generate_datapack(&self, duplicates: &[DuplicateGroup]) -> Result<Vec<u8>>;
}
```

---

## Модуль 5: Редактор рецептов

Новый модуль. Визуальное создание, изменение и удаление рецептов.

### Что делает

- **Конструктор крафтов** — интерфейс в виде верстака 3×3 с drag & drop предметов
- **Поддержка разных типов**: верстак, печь, наковальня, камнерез, станки из модов
- **Браузер всех рецептов** — фильтры по моду, типу станка, ингредиенту, результату
- **Поиск**: «Покажи все рецепты, где используется алмаз»
- **Отключение/изменение/замена** любого рецепта через UI
- **Программа генерирует скрипт** KubeJS / CraftTweaker / Data Pack JSON
- **Граф прогрессии** — визуальное дерево крафтов от базовых ресурсов до эндгейма, показывающее тупики и шорткаты

### Реализация в Rust

```rust
// crates/tuffbox-core/src/unified/recipe.rs
impl UnifiedRecipe {
    pub fn search_by_ingredient(item_id: &str, recipes: &[UnifiedRecipe]) -> Vec<Self>;
    pub fn search_by_result(item_id: &str, recipes: &[UnifiedRecipe]) -> Vec<Self>;
    pub fn to_kubejs6(&self) -> String;
    pub fn to_crafttweaker(&self) -> String;
    pub fn to_datapack_json(&self) -> Result<serde_json::Value>;
}

// crates/tuffbox-core/src/strategy/recipe_parser.rs
pub fn parse_recipe(json: &serde_json::Value, loader: LoaderKind, epoch: DataEpoch) -> Result<UnifiedRecipe>;
```

---

## Модуль 6: Редактор квестов FTB Quests

Новый модуль. Визуальное создание квестовых деревьев вне Minecraft.

### Что делает

- **Работает с файлами на диске (SNBT)**, не требует запуска Minecraft:
  - Парсинг: `config/ftbquests/quests/chapters/*.snbt` → внутренняя модель `QuestBook`
  - Сериализация: внутренняя модель → `.snbt` файлы на диске
- **Модель данных**:
  - `QuestBook → ChapterGroup[] → Chapter[] → Quest[] → Task[] + Reward[]`
  - Типы заданий: Item, Kill, Dimension, Biome, Stat, Checkmark, GameStage, Custom
  - Типы наград: Item, Xp, XpLevels, Command, RandomReward, Choice, Custom
  - QuestId — 16-символьные hex-строки, генерируются программой
- **Визуальный редактор** (d3-force):
  - Квестовые узлы — прямоугольники с иконкой и названием
  - Линии между узлами — зависимости
  - Drag & drop для перемещения квестов
  - Группировка по главам/вкладкам
  - Выбор предмета из каталога всех предметов установленных модов
- **Валидация**:
  - Зависимости указывают на существующие квесты
  - Нет циклических зависимостей
  - Все предметы в заданиях существуют в установленных модах
  - Каждый квест имеет хотя бы одно задание
  - Все квесты достижимы (есть путь от корня)
  - Нет дубликатов ID

### Реализация в Rust

```rust
// crates/tuffbox-core/src/unified/quest_book.rs
impl QuestBook {
    pub fn load_from_snbt(dir: &Path) -> Result<Self>;
    pub fn save_to_snbt(&self, dir: &Path) -> Result<()>;
    pub fn validate(&self, available_items: &[String]) -> Vec<QuestValidationError>;
    pub fn has_cycles(&self) -> bool;
    pub fn is_reachable(&self, quest_id: &str) -> bool;
}
```

---

## Модуль 7: Генерация мира

Новый модуль. Визуализация и настройка генерации руд и структур.

### Что делает

- **Вертикальный срез мира** (от бедрока до неба): цветные полосы показывают, какие руды на каких высотах
  - Данные собираются из конфигов модов через `knowledge::heuristics`
  - Для популярных модов — из `knowledge::builtin`
- **Интерактивное редактирование**: двигаешь полосу мышкой → программа обновляет конфиг
- **Менеджер структур**: список всех структур из модов, настройка частоты, предупреждения о конфликтах

### Реализация в Rust

```rust
// crates/tuffbox-core/src/knowledge/heuristics.rs
pub struct OreGeneration {
    pub mod_id: String,
    pub ore_name: String,
    pub min_height: i32,
    pub max_height: i32,
    pub vein_size: u32,
    pub frequency: u32,
    pub enabled: bool,
}

pub fn scan_ore_generation(configs: &[ConfigTree], knowledge: &ModKnowledge) -> Vec<OreGeneration>;
pub fn generate_ore_config_patch(ore: &OreGeneration, enabled: bool) -> Result<ConfigTree>;
```

---

## Модуль 8: AI-ассистент (платная фича)

Расширение текущего `Stage 11` (AI crash explanation).

### Что добавляется

- **Генератор скриптов** — пользователь пишет на естественном языке («убери крафт алмазной кирки, сделай из стали»), ИИ генерирует KubeJS / CraftTweaker скрипт
- **Генератор лора для квестов** — описания квестов в заданном стиле
- **Советчик по балансу** — ИИ анализирует граф прогрессии и указывает на дисбалансы
- **Переводчик** — перевод кастомных текстов (квесты, описания) на другие языки с учётом игрового контекста

### Реализация в Rust

```rust
// crates/tuffbox-core/src/knowledge/builtin.rs
// mod_knowledge.json расширяется для AI-контекста
pub struct ModKnowledgeEntry {
    pub config_paths: Vec<String>,
    pub ore_keys: Vec<String>,
    pub programmatic_items: Vec<String>,
    pub known_conflicts: Vec<String>,
    pub popularity_score: u32,
    pub ai_context: Option<String>,  // NEW: описание мода для ИИ
}
```

---

## Модуль 9: Тестирование

Расширение текущего `Stage 9` (test launcher).

### Что добавляется

- **Автоматические проверки без запуска игры**:
  - Валидация JSON-файлов (синтаксические ошибки)
  - Проверка, что все предметы в рецептах и квестах существуют
  - Проверка циклических зависимостей
  - Проверка, что нет сломанных ссылок
- **Чеклист тестирования** — автосгенерированный список того, что нужно проверить вручную

### Реализация в Rust

```rust
// crates/tuffbox-core/src/diagnostics.rs (расширение)
pub struct ValidationReport {
    pub json_errors: Vec<JsonError>,
    pub missing_items: Vec<MissingItem>,
    pub circular_deps: Vec<Vec<String>>,
    pub broken_references: Vec<BrokenReference>,
    pub checklist: Vec<ChecklistItem>,
}

impl ModpackProject {
    pub fn validate_offline(&self) -> Result<ValidationReport>;
}
```

---

## Модуль 10: Сборка и публикация

Расширение текущего `Stage 12` (export & release).

### Что добавляется

- **Генератор серверной сборки** (1 клик):
  - Копирует серверные моды, удаляет клиентские (по тегам + собственная база)
  - Генерирует `server.properties`, стартовый скрипт с JVM-флагами
  - Упаковывает в `.zip`
- **Экспорт для CurseForge / Modrinth** — автоматическое создание манифеста, changelog, README
- **Интеграция с хостингами** (монетизация): кнопка «Запустить сервер для тестирования с друзьями» → партнёрский хостинг

### Реализация в Rust

```rust
// crates/tuffbox-core/src/exporter.rs (расширение)
impl ModpackProject {
    pub fn export_server_pack(&self, output: &Path) -> Result<()>;
    pub fn export_curseforge_with_manifest(&self, output: &Path) -> Result<()>;
    pub fn export_modrinth_with_manifest(&self, output: &Path) -> Result<()>;
}
```

---

## Монетизация

| Источник | Модель | Ожидания |
|---|---|---|
| AI-ассистент | Подписка $4.99/мес | Покрывает API + прибыль |
| Партнёрство с хостингами | CPA (оплата за клиента) | BisectHosting, Apex и др. платят $10-30 за клиента |
| Программа | Бесплатная (open core) | Привлечение аудитории |
| Patreon / Boosty | Ранний доступ, голосование за фичи | Поддержка сообщества |

---

## Зависимости между модулями

```text
Unified Model Layer ← всё остальное зависит от этого
        │
        ├── Loader Adapters ← UnifiedMod, UnifiedRecipe, UnifiedTag
        │         │
        │         ├── Parsers ← ConfigTree
        │         │         │
        │         │         └── Knowledge ← ConfigTree + Heuristics
        │         │
        │         └── Strategy ← DataEpoch, TagNamespace, RecipeParser
        │
        ├── Модуль 1: Project ← ModpackProject, UnifiedMod
        ├── Модуль 2: Mod Manager ← UnifiedMod, LoaderAdapters
        ├── Модуль 3: Config Editor ← ConfigTree, Parsers, Knowledge
        ├── Модуль 4: Resource Unification ← UnifiedRecipe, UnifiedTag, DuplicateGroup
        ├── Модуль 5: Recipe Editor ← UnifiedRecipe, Strategy
        ├── Модуль 6: Quest Editor ← QuestBook, SnbtParser
        ├── Модуль 7: World Gen ← ConfigTree, Knowledge, OreGeneration
        ├── Модуль 8: AI ← UnifiedMod, ConfigTree, QuestBook, Knowledge
        ├── Модуль 9: Testing ← UnifiedMod, UnifiedRecipe, QuestBook
        └── Модуль 10: Build & Publish ← UnifiedMod, ModpackProject
```

### Порядок реализации

1. **Unified Model Layer** — типы, от которых всё зависит
2. **Loader Adapters + Parsers** — парсинг JAR и конфигов
3. **Strategy + Knowledge** — версии, эпохи, эвристики
4. **Модули 1-3** — управление проектом, модами, конфигами
5. **Модули 4-6** — унификация, рецепты, квесты
6. **Модули 7-10** — мир, AI, тестирование, сборка
