/**
 * Minimal RU/EN localization for the app shell and library surfaces.
 *
 * The backend ships a small L10N map (`localize` / `list_localizations`
 * commands), but a synchronous front-end dictionary is the better transport:
 * no IPC latency on every label, trivially extensible, and testable in node.
 * The backend commands stay available but are no longer the plan of record.
 *
 * Coverage grows incrementally — `t()` falls back to the English string and
 * then to the key itself, so untranslated surfaces keep working unchanged.
 */
import { derived, writable } from "svelte/store";

export type Locale = "ru" | "en";

const LOCALE_KEY = "tuffbox.locale";

export const LOCALES: { id: Locale; label: string }[] = [
  { id: "en", label: "EN" },
  { id: "ru", label: "RU" },
];

function readStoredLocale(): Locale {
  try {
    const raw = localStorage.getItem(LOCALE_KEY);
    if (raw === "ru" || raw === "en") return raw;
  } catch {
    /* unavailable outside the webview */
  }
  return "en";
}

/** Active UI language; persisted to localStorage. */
export const locale = writable<Locale>(readStoredLocale());

locale.subscribe((l) => {
  try {
    localStorage.setItem(LOCALE_KEY, l);
  } catch {
    /* ignore */
  }
});

type Entry = { en: string; ru: string };

/** Dictionary — dotted keys, `{token}` placeholders filled by `t()`. */
const dict: Record<string, Entry> = {
  // ── Common verbs / nouns ──
  "common.play": { en: "Play", ru: "Играть" },
  "common.stop": { en: "Stop", ru: "Стоп" },
  "common.settings": { en: "Settings", ru: "Настройки" },
  "common.help": { en: "Help", ru: "Помощь" },
  "common.refresh": { en: "Refresh", ru: "Обновить" },
  "common.export": { en: "Export", ru: "Экспорт" },
  "common.more": { en: "More", ru: "Ещё" },
  "common.apply": { en: "Apply", ru: "Применить" },
  "common.cancel": { en: "Cancel", ru: "Отмена" },
  "common.save": { en: "Save", ru: "Сохранить" },
  "common.close": { en: "Close", ru: "Закрыть" },
  "common.delete": { en: "Delete", ru: "Удалить" },
  "common.done": { en: "Done", ru: "Готово" },
  "common.copy": { en: "Copy", ru: "Копировать" },

  // ── Sidebar rail ──
  "nav.home": { en: "Java Edition", ru: "Java Edition" },
  "nav.library": { en: "Library", ru: "Библиотека" },
  "nav.ide": { en: "IDE", ru: "IDE" },
  "nav.addInstance": { en: "Add instance", ru: "Добавить сборку" },
  "nav.logs": { en: "Logs", ru: "Логи" },
  "nav.settings": { en: "Settings", ru: "Настройки" },
  "nav.profile": { en: "Profile", ru: "Профиль" },
  "nav.app": { en: "App", ru: "Приложение" },

  // ── Library toolbar ──
  "library.title": { en: "Library", ru: "Библиотека" },
  "library.addInstance": { en: "Add Instance", ru: "Добавить сборку" },
  "library.addInstanceTitle": {
    en: "Add an instance to the library",
    ru: "Добавить сборку в библиотеку",
  },
  "library.folders": { en: "Folders", ru: "Папки" },
  "library.update": { en: "Update", ru: "Обновить" },
  "library.account": { en: "Account", ru: "Аккаунт" },
  "library.filterPlaceholder": { en: "Filter instances…", ru: "Фильтр сборок…" },
  "library.filterAria": { en: "Filter instances", ru: "Фильтр сборок" },
  "library.clearFilter": { en: "Clear filter", ru: "Сбросить фильтр" },
  "library.sort": { en: "Sort instances", ru: "Сортировка сборок" },
  "library.sortRecent": { en: "Last played", ru: "Недавние" },
  "library.sortName": { en: "Name", ru: "По имени" },
  "library.sortPlaytime": { en: "Most played", ru: "По времени игры" },
  "library.layout": { en: "Layout", ru: "Вид" },
  "library.gridView": { en: "Grid view", ru: "Плитка" },
  "library.listView": { en: "List view", ru: "Список" },

  // ── Library empty states ──
  "library.emptyTitle": { en: "No instances yet", ru: "Пока нет сборок" },
  "library.emptyBody": {
    en: "Create or import a pack to build your library.",
    ru: "Создайте или импортируйте сборку, чтобы наполнить библиотеку.",
  },
  "library.noMatchesTitle": { en: "No matches", ru: "Ничего не найдено" },
  "library.noMatchesBody": {
    en: "Nothing matches “{filter}”. Try another name, version or loader.",
    ru: "По запросу «{filter}» ничего нет. Попробуйте другое имя, версию или загрузчик.",
  },

  // ── Tile / row actions ──
  "library.playAria": { en: "Play {name}", ru: "Играть: {name}" },
  "library.stopAria": { en: "Stop {name}", ru: "Остановить {name}" },
  "library.openInIde": { en: "Open in IDE", ru: "Открыть в IDE" },
  "library.openFolder": { en: "Open folder", ru: "Открыть папку" },
  "library.lastPlayed": { en: "Last played", ru: "Последний запуск" },
  "library.playtime": { en: "Total playtime", ru: "Всего в игре" },
  "library.playStats": { en: "Play statistics", ru: "Статистика игры" },
  "library.changeGroup": { en: "Change group", ru: "Сменить группу" },

  // ── Update center ──
  "library.updatesTitle": {
    en: "Update every mod with a newer release",
    ru: "Обновить все моды до свежих версий",
  },
  "library.modsHaveUpdates": {
    en: "{n} mods have updates",
    ru: "Модов с обновлениями: {n}",
  },
  "library.modHasUpdate": { en: "1 mod has updates", ru: "1 мод с обновлением" },
  "library.updateAll": { en: "Update all", ru: "Обновить всё" },
  "library.updatesBadgeTitle": {
    en: "{n} mods have updates",
    ru: "Модов с обновлениями: {n}",
  },

  // ── Instance side panel ──
  "library.manageInstance": { en: "Manage instance", ru: "Управление сборкой" },
  "library.manageEllipsis": { en: "Manage…", ru: "Управлять…" },
  "library.folder": { en: "Folder", ru: "Папка" },
  "library.manage": { en: "Manage", ru: "Управлять" },
  "library.moreActions": { en: "More actions", ru: "Ещё действия" },
  "library.sideMeta": { en: "Instance actions", ru: "Действия сборки" },
  "library.playTime": { en: "Play time", ru: "Время в игре" },
  "library.java": { en: "Java", ru: "Java" },
  "library.memory": { en: "Memory", ru: "Память" },
  "library.notes": { en: "Notes", ru: "Заметки" },
  "library.notesPlaceholder": {
    en: "Reminders, TODOs, server IPs… (saved automatically)",
    ru: "Напоминания, планы, адреса серверов… (сохраняется автоматически)",
  },
  "library.content": { en: "Content", ru: "Содержимое" },
  "library.selectInstance": {
    en: "Select an instance to see its details",
    ru: "Выберите сборку, чтобы увидеть подробности",
  },
  "library.modsBackupsHealth": {
    en: "Mods, backups and health for this pack",
    ru: "Моды, бэкапы и состояние сборки",
  },
  "library.openIdeAria": { en: "Open in IDE", ru: "Открыть в IDE" },
  "library.openFolderAria": { en: "Open the instance folder", ru: "Открыть папку сборки" },

  // ── Group dialog ──
  "library.groupDialogTitle": { en: "Change Group", ru: "Смена группы" },
  "library.groupDialogBody": {
    en: "Move “{name}” into a group.",
    ru: "Переместить «{name}» в группу.",
  },
  "library.groupOrNew": { en: "Or type a new name", ru: "Или введите новое имя" },

  // ── Header action buttons ──
  "library.renameInstance": { en: "Rename instance", ru: "Переименовать сборку" },
  "library.copyInstance": { en: "Copy instance", ru: "Копировать сборку" },
  "library.importGithub": { en: "Import from GitHub", ru: "Импорт с GitHub" },
  "library.installGithubPack": { en: "Install GitHub pack", ru: "Установить пак с GitHub" },

  // ── Language switcher ──
  "nav.language": { en: "Language", ru: "Язык" },

  // ── Home hero / shelf ──
  "home.playAria": { en: "Play", ru: "Играть" },
  "home.instance": { en: "Instance", ru: "Сборка" },
  "home.instanceSettings": { en: "Instance settings", ru: "Настройки сборки" },
  "home.folder": { en: "Folder", ru: "Папка" },
  "home.openInstanceFolder": { en: "Open instance folder", ru: "Открыть папку сборки" },
  "home.moreActions": { en: "More instance actions", ru: "Ещё действия сборки" },
  "home.moreTitle": {
    en: "Rename, clone, export, repair, delete…",
    ru: "Переименовать, клонировать, экспортировать, починить, удалить…",
  },
  "home.rename": { en: "Rename", ru: "Переименовать" },
  "home.clone": { en: "Clone", ru: "Клонировать" },
  "home.exportMrpack": { en: "Export .mrpack", ru: "Экспорт .mrpack" },
  "home.serverPack": { en: "Server pack", ru: "Серверный пак" },
  "home.repair": { en: "Repair", ru: "Починить" },
  "home.logsZip": { en: "Logs .zip", ru: "Логи .zip" },
  "home.emptyHint": {
    en: "Create a blank pack, import one you already have, or browse the library.",
    ru: "Создайте чистую сборку, импортируйте готовую или загляните в библиотеку.",
  },
  "home.selectTitle": { en: "Select an instance", ru: "Выберите сборку" },
  "home.selectHint": {
    en: "Pick a pack from the shelf below, or create a new instance.",
    ru: "Выберите сборку с полки ниже или создайте новую.",
  },
  "home.create": { en: "Create", ru: "Создать" },
  "home.import": { en: "Import", ru: "Импорт" },
  "home.browse": { en: "Browse", ru: "Обзор" },
  "home.fixApplied": { en: "Fix applied", ru: "Исправление применено" },
  "home.fixSoftVerify": {
    en: "Play about {n}s more to confirm it works.",
    ru: "Поиграйте ещё ~{n} с, чтобы убедиться, что всё работает.",
  },
  "home.fixLaunch": {
    en: "Launch the game to confirm the fix. You can restore anytime.",
    ru: "Запустите игру, чтобы подтвердить исправление. Откат возможен в любой момент.",
  },
  "home.diagnostics": { en: "Diagnostics", ru: "Диагностика" },
  "home.launching": { en: "Launching…", ru: "Запуск…" },
  "home.shelfEmpty": {
    en: "Your shelf is empty. Create a pack or import one you already have.",
    ru: "Полка пуста. Создайте сборку или импортируйте готовую.",
  },
  "home.running": { en: "Running", ru: "Запущена" },

  // ── Instance manager ──
  "manager.closeManager": { en: "Close manager", ru: "Закрыть менеджер" },
  "manager.sections": { en: "Manager sections", ru: "Разделы менеджера" },
  "manager.updatesAvailable": { en: "Updates available", ru: "Есть обновления" },
  "manager.mods": { en: "Mods", ru: "Моды" },
  "manager.worlds": { en: "Worlds", ru: "Миры" },
  "manager.screenshots": { en: "Screenshots", ru: "Скриншоты" },
  "manager.backups": { en: "Backups", ru: "Бэкапы" },
  "manager.health": { en: "Health", ru: "Состояние" },
  "manager.filterMods": { en: "Filter mods…", ru: "Фильтр модов…" },
  "manager.filterModsAria": { en: "Filter mods", ru: "Фильтр модов" },
  "manager.sortMods": { en: "Sort mods", ru: "Сортировка модов" },
  "manager.sortName": { en: "Name", ru: "Имя" },
  "manager.sortSource": { en: "Source", ru: "Источник" },
  "manager.sortUpdates": { en: "Updates first", ru: "Сначала обновления" },
  "manager.refreshMods": { en: "Refresh mod list", ru: "Обновить список модов" },
  "manager.moreModActions": { en: "More mod actions", ru: "Ещё действия" },
  "manager.checkUpdates": { en: "Check for updates", ru: "Проверить обновления" },
  "manager.updateAll": { en: "Update all", ru: "Обновить всё" },
  "manager.addMods": { en: "Add mods…", ru: "Добавить моды…" },
  "manager.syncFolder": { en: "Sync mods folder", ru: "Синхронизировать папку модов" },
  "manager.openModsFolder": { en: "Open mods folder", ru: "Открыть папку модов" },
  "manager.batchActions": { en: "Batch mod actions", ru: "Массовые действия" },
  "manager.selected": { en: "{n} selected", ru: "Выбрано: {n}" },
  "manager.enable": { en: "Enable", ru: "Включить" },
  "manager.disable": { en: "Disable", ru: "Выключить" },
  "manager.update": { en: "Update", ru: "Обновить" },
  "manager.updateN": { en: "Update {n}", ru: "Обновить: {n}" },
  "manager.remove": { en: "Remove", ru: "Убрать" },
  "manager.noUpdatesSelected": {
    en: "No updates among selected mods",
    ru: "Среди выбранных нет обновлений",
  },
  "manager.all": { en: "All", ru: "Все" },
  "manager.clear": { en: "Clear", ru: "Сброс" },
  "manager.quickFilters": { en: "Quick filters", ru: "Быстрые фильтры" },
  "manager.allMods": { en: "All mods", ru: "Все моды" },
  "manager.clearQuickFilter": { en: "Clear the quick filter", ru: "Сбросить быстрый фильтр" },
  "manager.modWord": { en: "mod", ru: "мод" },
  "manager.modsWord": { en: "mods", ru: "модов" },
  "manager.disabledWord": { en: "disabled", ru: "отключено" },
  "manager.showDisabled": { en: "Show only disabled mods", ru: "Только отключённые" },
  "manager.showingDisabled": {
    en: "Showing only disabled mods — click to clear",
    ru: "Показаны только отключённые — клик сбрасывает",
  },
  "manager.showUpdates": { en: "Show only mods with updates", ru: "Только с обновлениями" },
  "manager.showingUpdates": {
    en: "Showing only mods with updates — click to clear",
    ru: "Показаны только моды с обновлениями — клик сбрасывает",
  },
  "manager.upToDate": { en: "Up to date", ru: "Актуальны" },
  "manager.updatesCount": { en: "{n} update(s)", ru: "обновлений: {n}" },
  "manager.readingMods": { en: "Reading the mod list…", ru: "Читаю список модов…" },
  "manager.modsError": { en: "Couldn't read the mod list.", ru: "Не удалось прочитать список модов." },
  "manager.noModsMatch": { en: "No mods match \"{filter}\".", ru: "Нет модов по запросу «{filter}»." },
  "manager.noMods": {
    en: "No mods yet — browse the catalog to add some.",
    ru: "Модов пока нет — откройте каталог, чтобы добавить.",
  },
  "manager.openCatalog": { en: "Open catalog", ru: "Открыть каталог" },
  "manager.changeVersion": { en: "Change version…", ru: "Сменить версию…" },
  "manager.removeMod": { en: "Remove mod", ru: "Убрать мод" },
  "manager.removeMods": { en: "Remove mods", ru: "Убрать моды" },
  "manager.removeModMsg": {
    en: "Remove \"{name}\" from this instance? The jar is deleted from the mods folder.",
    ru: "Убрать «{name}» из сборки? Jar-файл будет удалён из папки mods.",
  },
  "manager.removeModsMsg": {
    en: "Remove {n} selected {word} from this instance?",
    ru: "Убрать выбранные моды ({n}) из этой сборки?",
  },
  "manager.worldsHint": {
    en: "World saves live in saves/, with per-world zip backups.",
    ru: "Сохранения миров лежат в saves/, zip-бэкапы делаются по каждому миру.",
  },
  "manager.refreshWorlds": { en: "Refresh worlds", ru: "Обновить миры" },
  "manager.readingWorlds": { en: "Reading saves…", ru: "Читаю сохранения…" },
  "manager.launchWorld": {
    en: "Launch the instance and join this world",
    ru: "Запустить сборку и войти в этот мир",
  },
  "manager.zipWorld": {
    en: "Zip this world into per-world backups",
    ru: "Заархивировать этот мир в бэкапы",
  },
  "manager.deleteWorld": { en: "Delete world", ru: "Удалить мир" },
  "manager.deleteWorldTitle": {
    en: "Delete world (a backup is kept automatically)",
    ru: "Удалить мир (бэкап сохранится автоматически)",
  },
  "manager.deleteWorldAria": { en: "Delete world {name}", ru: "Удалить мир {name}" },
  "manager.deleteWorldMsg": {
    en: "Delete \"{name}\"? A zip backup is kept automatically before deletion.",
    ru: "Удалить «{name}»? Перед удалением автоматически сохранится zip-бэкап.",
  },
  "manager.shotsHint": {
    en: "Press F2 in game to take a screenshot.",
    ru: "Скриншот в игре делается клавишей F2.",
  },
  "manager.refreshShots": { en: "Refresh screenshots", ru: "Обновить скриншоты" },
  "manager.listingShots": { en: "Listing screenshots…", ru: "Загружаю скриншоты…" },
  "manager.preview": { en: "Preview", ru: "Просмотр" },
  "manager.deleteShot": { en: "Delete screenshot", ru: "Удалить скриншот" },
  "manager.deleteShotMsg": {
    en: "Delete \"{name}\"? This cannot be undone.",
    ru: "Удалить «{name}»? Это действие нельзя отменить.",
  },
  "manager.openInViewer": { en: "Open in viewer", ru: "Открыть в просмотрщике" },
  "manager.backupsHint": {
    en: "Snapshots the whole instance folder.",
    ru: "Снапшоты всей папки сборки.",
  },
  "manager.refreshBackups": { en: "Refresh backups", ru: "Обновить бэкапы" },
  "manager.loadingBackups": { en: "Loading backups…", ru: "Загружаю бэкапы…" },
  "manager.restore": { en: "Restore", ru: "Восстановить" },
  "manager.deleteBackup": { en: "Delete backup", ru: "Удалить бэкап" },
  "manager.restoreBackup": { en: "Restore backup", ru: "Восстановление бэкапа" },
  "manager.restoreBackupMsg": {
    en: "Replace the current instance state with \"{name}\" ({stamp})? A backup of the current state is kept.",
    ru: "Заменить текущее состояние сборки на «{name}» ({stamp})? Бэкап текущего состояния сохранится.",
  },
  "manager.deleteBackupMsg": {
    en: "Delete backup \"{name}\"? This cannot be undone.",
    ru: "Удалить бэкап «{name}»? Это действие нельзя отменить.",
  },
  "manager.healthHint": {
    en: "Manifest, graph and config checks.",
    ru: "Проверка манифеста, графа и конфигов.",
  },
  "manager.refreshHealth": { en: "Refresh health report", ru: "Обновить отчёт" },
  "manager.scanningPack": { en: "Scanning the pack…", ru: "Сканирую сборку…" },
  "manager.healthError": {
    en: "Couldn't load the health report.",
    ru: "Не удалось загрузить отчёт о состоянии.",
  },
  "manager.wrongLoader": {
    en: "Mods built for a different loader",
    ru: "Моды от другого загрузчика",
  },
  "manager.wrongLoaderWord": { en: "wrong loader", ru: "чужой загрузчик" },
  "manager.duplicateJarsTitle": { en: "Duplicate mod jars", ru: "Дубликаты jar-файлов" },
  "manager.dupGroups": { en: "dup groups", ru: "групп дублей" },
  "manager.questIssuesTitle": { en: "Quest book issues", ru: "Проблемы книги квестов" },
  "manager.questIssuesWord": { en: "quest issues", ru: "проблем квестов" },
  "manager.recentCrash": { en: "recent crash", ru: "недавний краш" },
  "manager.exportIssues": { en: "Export issues", ru: "Проблемы экспорта" },
  "manager.duplicateJars": { en: "Duplicate jars", ru: "Дубликаты jar" },
  "manager.lastValidation": { en: "Last validation", ru: "Последняя проверка" },
  "manager.andMore": { en: "…and {n} more", ru: "…и ещё {n}" },
  "manager.jarsOf": { en: "{n} jars of {mod}", ru: "{n} jar-файлов: {mod}" },
  "manager.keepRest": {
    en: "Keep {name}, delete the rest",
    ru: "Оставить {name}, удалить остальные",
  },
  "manager.keep": { en: "Keep {name}", ru: "Оставить {name}" },
  "manager.validationPassed": { en: "Validation passed", ru: "Проверка пройдена" },
  "manager.issuesFound": { en: "Issues found", ru: "Найдены проблемы" },
  "manager.changeVersionTitle": { en: "Change version", ru: "Смена версии" },
  "manager.loadingVersions": { en: "Loading versions…", ru: "Загружаю версии…" },
  "manager.install": { en: "Install", ru: "Установить" },

  // ── Settings ──
  "settings.sections": { en: "Settings sections", ru: "Разделы настроек" },
  "settings.tab.appearance": { en: "Appearance", ru: "Внешний вид" },
  "settings.tab.launcher": { en: "Launcher", ru: "Лаунчер" },
  "settings.tab.ai": { en: "AI", ru: "AI" },
  "settings.tab.integrations": { en: "Integrations", ru: "Интеграции" },
  "settings.tab.about": { en: "About", ru: "О программе" },
  "settings.subnav": { en: "Launcher settings", ru: "Настройки лаунчера" },
  "settings.sub.general": { en: "General", ru: "Общие" },
  "settings.sub.java": { en: "Java", ru: "Java" },
  "settings.sub.commands": { en: "Commands", ru: "Команды" },
  "settings.sub.runtime": { en: "Paths", ru: "Пути" },
  "settings.sub.storage": { en: "Storage", ru: "Хранилище" },
  "settings.appearance": { en: "Appearance", ru: "Внешний вид" },
  "settings.theme": { en: "Theme", ru: "Тема" },
  "settings.themeHint": {
    en: "Hover a swatch to preview — click to save.",
    ru: "Наведите на образец для предпросмотра — клик сохраняет выбор.",
  },
  "settings.potato": {
    en: "Potato PC mode (reduce motion / animations)",
    ru: "Режим слабого ПК (меньше движения и анимаций)",
  },
  "settings.potatoHint": {
    en: "Disables CSS animations and transitions for weaker machines.",
    ru: "Отключает CSS-анимации и переходы на слабых машинах.",
  },
  "settings.glass": { en: "Glass transparency", ru: "Стеклянная прозрачность" },
  "settings.glassHint": {
    en: "See-through cards, sidebar and header with backdrop blur over the theme background — works with every theme. Off by default.",
    ru: "Прозрачные карточки, сайдбар и шапка с размытием поверх фона темы — работает с любой темой. Выкл. по умолчанию.",
  },
  "settings.rounded": { en: "Rounded corners", ru: "Скруглённые углы" },
  "settings.roundedHint": {
    en: "Round edges on panels, cards, modals, and chrome — works with every theme.",
    ru: "Скругление панелей, карточек, модалок и элементов — работает с любой темой.",
  },
  "settings.backdrop": { en: "Home backdrop", ru: "Фон главного экрана" },
  "settings.backdropHint": {
    en: "Polished stone backdrop panel behind the home dashboard (home only).",
    ru: "Каменная панель фона за дашбордом (только главный экран).",
  },
  "settings.uiScale": { en: "Interface scale", ru: "Масштаб интерфейса" },
  "settings.uiScaleDescA": {
    en: "Zoom the whole UI — buttons, sidebar, Content mod cards, dialogs.",
    ru: "Масштабирует весь интерфейс — кнопки, сайдбар, карточки модов, диалоги.",
  },
  "settings.auto": { en: "Auto", ru: "Авто" },
  "settings.uiScaleDescB": {
    en: "picks a size from your screen and window; pick a percent to lock it.",
    ru: "подбирает размер по экрану и окну; процент фиксирует его.",
  },
  "settings.suggestedScale": {
    en: "Suggested for this screen: {n}%",
    ru: "Для этого экрана подходит: {n}%",
  },
  "settings.followingWindow": { en: "· following window size", ru: "· следует за размером окна" },
  "settings.ytFeed": { en: "YouTube feed on home", ru: "YouTube-лента на главном" },
  "settings.ytFeedDesc": {
    en: "Minecraft YouTube strip on the home screen. Hidden by default — turn it on here or via the feed settings button on the home banner.",
    ru: "Полоска Minecraft-YouTube на главном экране. По умолчанию скрыта — включается здесь или в настройках ленты на баннере.",
  },
  "settings.shown": { en: "Shown", ru: "Показана" },
  "settings.hidden": { en: "Hidden", ru: "Скрыта" },
  "settings.ytPlayer": { en: "YouTube player", ru: "YouTube-плеер" },
  "settings.ytPlayerDesc": {
    en: "Litube-style in-app player loads a privacy embed only after you click a thumbnail. Preview-only keeps static images and opens videos in the system browser.",
    ru: "Встроенный плеер подгружает privacy-embed только после клика по превью. Режим «только превью» показывает статичные картинки и открывает видео в браузере.",
  },
  "settings.inAppPlayer": { en: "In-app player", ru: "Встроенный плеер" },
  "settings.previewOnly": { en: "Preview only", ru: "Только превью" },
  "settings.overlay": { en: "In-game overlay", ru: "Внутриигровой оверлей" },
  "settings.overlayDescA": {
    en: "F8 fullscreen overlay (OpenGL hook) — any MC version / loader. Friends, chat, YouTube feed via launcher IPC. Place",
    ru: "F8-оверлей на весь экран (OpenGL hook) — любая версия/загрузчик MC. Друзья, чат, YouTube-лента через IPC лаунчера. Положите",
  },
  "settings.overlayDescB": { en: "next to the hook for video.", ru: "рядом с хуком для видео." },
  "settings.enabled": { en: "Enabled", ru: "Вкл" },
  "settings.disabled": { en: "Disabled", ru: "Выкл" },
  "settings.dynPanel": { en: "Dynamic bottom panel", ru: "Динамическая нижняя панель" },
  "settings.dynPanelDesc": {
    en: "Hide the IDE workflow rail (Content, Setup, …). Move the cursor to the bottom edge of the window to slide it out quickly; it hides again when you leave.",
    ru: "Скрывает рейл IDE (Content, Setup, …). Подведите курсор к нижнему краю окна, чтобы быстро вытащить панель; при уходе курсора она скрывается.",
  },
  "settings.autoHide": { en: "Auto-hide", ru: "Автоскрытие" },
  "settings.alwaysVisible": { en: "Always visible", ru: "Всегда виден" },
  "settings.idePanel": { en: "IDE top panel", ru: "Верхняя панель IDE" },
  "settings.idePanelDesc": {
    en: "Hide the strip at the top of the IDE (pack status, suggested next step, Health check) to give the workspace more room. Shortcuts and the command palette keep working while it is hidden.",
    ru: "Скрывает полосу сверху IDE (статус сборки, следующий шаг, проверка состояния), освобождая место рабочей области. Хоткеи и палитра команд работают и при скрытой панели.",
  },
  "settings.concDownloads": { en: "Concurrent downloads", ru: "Параллельные загрузки" },
  "settings.concDownloadsDesc": {
    en: "How many files to fetch in parallel when installing mods or updating the instance.",
    ru: "Сколько файлов качать параллельно при установке или обновлении модов.",
  },
  "settings.resolution": { en: "Game resolution", ru: "Разрешение игры" },
  "settings.resolutionDesc": {
    en: "Window size passed to Minecraft on launch. Leave Default to use the game's own setting.",
    ru: "Размер окна, передаваемый Minecraft при запуске. Оставьте «По умолчанию», чтобы использовать настройку игры.",
  },
  "settings.default": { en: "Default", ru: "По умолчанию" },
  "settings.custom": { en: "Custom", ru: "Свой" },
  "settings.width": { en: "Width", ru: "Ширина" },
  "settings.height": { en: "Height", ru: "Высота" },
  "settings.discord": { en: "Discord Rich Presence", ru: "Discord Rich Presence" },
  "settings.discordDesc": {
    en: "Show what you're playing in Discord while Minecraft is running. Needs an Application Client ID from the Discord Developer Portal.",
    ru: "Показывает, во что вы играете, в Discord, пока запущен Minecraft. Нужен Application Client ID из Discord Developer Portal.",
  },
  "settings.enableRpc": { en: "Enable Rich Presence", ru: "Включить Rich Presence" },
  "settings.clientId": { en: "Application Client ID", ru: "Application Client ID" },
  "settings.clientIdPlaceholder": {
    en: "Application ID from Discord Developer Portal",
    ru: "Application ID из Discord Developer Portal",
  },
  "settings.portal": { en: "Portal", ru: "Портал" },
  "settings.portalTitle": {
    en: "Open Discord Developer Portal",
    ru: "Открыть Discord Developer Portal",
  },
  "settings.saving": { en: "Saving…", ru: "Сохранение…" },
  "settings.savePresence": { en: "Save presence", ru: "Сохранить presence" },
  "settings.discordHintA": {
    en: "Optional: upload a large image asset named",
    ru: "Опционально: загрузите в приложении Discord крупную картинку-ассет с именем",
  },
  "settings.discordHintB": {
    en: "in the Discord app for a richer status card.",
    ru: "— карточка статуса будет выглядеть богаче.",
  },
  "settings.shortcuts": { en: "Keyboard shortcuts", ru: "Горячие клавиши" },
  "settings.shortcutsDesc": {
    en: "Built-in hotkeys for navigating TuffBox.",
    ru: "Встроенные хоткеи для навигации в TuffBox.",
  },
  "settings.show": { en: "Show", ru: "Показать" },
  "settings.hide": { en: "Hide", ru: "Скрыть" },
  "settings.shortcutsWord": { en: "shortcuts", ru: "сочетаний" },
  "settings.general": { en: "General", ru: "Общие" },
  "settings.java": { en: "Java", ru: "Java" },
  "settings.defaultJavaPath": { en: "Default Java path", ru: "Путь к Java по умолчанию" },
  "settings.autoDetect": { en: "Auto-detect", ru: "Автоопределение" },
  "settings.browse": { en: "Browse…", ru: "Обзор…" },
  "settings.customArgs": { en: "Custom Java arguments", ru: "Свои аргументы Java" },
  "settings.customArgsHint": {
    en: "Extra options passed to Java when the game starts (garbage collector tuning and similar). Leave empty if unsure — TuffBox already picks sensible defaults.",
    ru: "Дополнительные опции для Java при запуске игры (настройка сборщика мусора и т.п.). Не уверены — оставьте пустым: TuffBox уже подбирает разумные значения.",
  },
  "settings.cpuAffinity": { en: "CPU affinity", ru: "Привязка к CPU" },
  "settings.affinityOff": { en: "Off (let Windows decide)", ru: "Выкл (пусть решает Windows)" },
  "settings.affinityPerf": {
    en: "Performance cores (hybrid CPUs)",
    ru: "Производительные ядра (гибридные CPU)",
  },
  "settings.affinityManual": { en: "Manual mask", ru: "Ручная маска" },
  "settings.affinityHint": {
    en: "Chooses which CPU cores the game runs on. Leave \"Off\" if unsure — \"Performance cores\" only helps on hybrid CPUs (Intel 12th gen+); \"Manual mask\" is for advanced users.",
    ru: "Выбирает, на каких ядрах CPU работает игра. Не уверены — оставьте «Выкл»; «Производительные ядра» помогают только на гибридных CPU (Intel 12-го поколения и новее); «Ручная маска» — для продвинутых.",
  },
  "settings.affinityMask": { en: "Affinity mask (hex)", ru: "Маска affinity (hex)" },
  "settings.gpu": { en: "GPU", ru: "GPU" },
  "settings.gpuAuto": {
    en: "Auto (discrete when available)",
    ru: "Авто (дискретная, если есть)",
  },
  "settings.gpuDiscrete": { en: "Discrete GPU", ru: "Дискретная GPU" },
  "settings.gpuIntegrated": { en: "Integrated GPU", ru: "Встроенная GPU" },
  "settings.redetectGpus": { en: "Re-detect GPUs", ru: "Переопределить GPU" },
  "settings.detecting": { en: "Detecting…", ru: "Определяю…" },
  "settings.detect": { en: "Detect", ru: "Определить" },
  "settings.memory": { en: "Default memory (MB)", ru: "Память по умолчанию (МБ)" },
  "settings.autoTuneTitle": {
    en: "Pick heap size from total RAM and mod count",
    ru: "Подобрать heap по объёму RAM и числу модов",
  },
  "settings.measuring": { en: "Measuring…", ru: "Измеряю…" },
  "settings.saveJava": { en: "Save Java settings", ru: "Сохранить настройки Java" },
  "settings.launchCommands": { en: "Launch commands", ru: "Команды запуска" },
  "settings.preLaunch": { en: "Run before the game starts", ru: "Команда перед запуском игры" },
  "settings.preLaunchHint": {
    en: "Optional. Runs once right before Minecraft launches.",
    ru: "Опционально. Выполняется один раз прямо перед запуском Minecraft.",
  },
  "settings.preLaunchPlaceholder": {
    en: "Optional command, e.g. a backup script",
    ru: "Необязательная команда, напр. скрипт бэкапа",
  },
  "settings.postExit": { en: "Run after the game closes", ru: "Команда после выхода из игры" },
  "settings.postExitHint": {
    en: "Optional. Runs once after you quit the game.",
    ru: "Опционально. Выполняется один раз после выхода из игры.",
  },
  "settings.postExitPlaceholder": {
    en: "Optional command, e.g. cleanup",
    ru: "Необязательная команда, напр. очистка",
  },
  "settings.wrapper": { en: "Wrapper command", ru: "Команда-обёртка" },
  "settings.wrapperHint": {
    en: "Optional. Starts the game through this command (advanced).",
    ru: "Опционально. Запускает игру через эту команду (для продвинутых).",
  },
  "settings.wrapperPlaceholder": {
    en: "e.g. gamemoderun (Linux)",
    ru: "напр. gamemoderun (Linux)",
  },
  "settings.runtimePath": { en: "Runtime path", ru: "Пути" },
  "settings.runtimeHint": {
    en: "Move the shared runtime (libraries, assets, Java) to another disk to free space on the system drive. Default:",
    ru: "Перенесите общий рантайм (библиотеки, ассеты, Java) на другой диск, освободив системный. По умолчанию:",
  },
  "settings.runtimeDir": { en: "Runtime directory", ru: "Каталог рантайма" },
  "settings.applyPath": { en: "Apply path", ru: "Применить путь" },
  "settings.resetDefault": { en: "Reset to default", ru: "Вернуть по умолчанию" },
  "settings.instancesFolder": {
    en: "Modpacks / instances folder",
    ru: "Папка сборок",
  },
  "settings.instancesHint": {
    en: "Where Discover and Add Instance download modpacks by default. Default:",
    ru: "Куда Discover и «Добавить сборку» скачивают сборки по умолчанию. По умолчанию:",
  },
  "settings.downloadDir": { en: "Download directory", ru: "Каталог загрузок" },
  "settings.dedupStore": { en: "Dedup store", ru: "Дедуп-хранилище" },
  "settings.dedupDesc": {
    en: "TuffBox stores identical mod jars, resourcepacks, shaderpacks, libraries and game files once on disk and hard-links them into every project that uses them.",
    ru: "TuffBox хранит одинаковые jar модов, ресурспаки, шейдерпаки, библиотеки и файлы игры на диске один раз и хардлинками подключает их к каждому проекту.",
  },
  "settings.dedupCounts": {
    en: "{n} shared file(s), {size} on disk.",
    ru: "Общих файлов: {n}, на диске: {size}.",
  },
  "settings.dedupRun": {
    en: "Deduplicate existing projects",
    ru: "Дедуплицировать существующие проекты",
  },
  "settings.working": { en: "Working…", ru: "Работаю…" },
  "settings.dedupClean": {
    en: "Clean unused store files",
    ru: "Очистить неиспользуемые файлы",
  },
  "settings.ai": { en: "AI", ru: "AI" },
  "settings.aiDesc": {
    en: "TuffBox can use AI to explain crashes and suggest fixes. Run it locally with Ollama (private, works offline) or connect a cloud API with a key. Advanced options live below.",
    ru: "TuffBox умеет объяснять краши и предлагать исправления с помощью AI. Локально через Ollama (приватно, работает офлайн) или облачный API с ключом. Продвинутые опции — ниже.",
  },
  "settings.integrations": { en: "Integrations", ru: "Интеграции" },
  "settings.loadingIntegrations": {
    en: "Loading integration status…",
    ru: "Загружаю статус интеграций…",
  },
  "settings.defaultRepo": {
    en: "Default repository (owner/name)",
    ru: "Репозиторий по умолчанию (owner/name)",
  },
  "settings.pat": { en: "Personal access token", ru: "Персональный токен доступа" },
  "settings.apiToken": { en: "API token", ru: "API-токен" },
  "settings.saveToken": { en: "Save token", ru: "Сохранить токен" },
  "settings.tokenPlaceholder": {
    en: "•••••••• (enter new to replace)",
    ru: "•••••••• (введите новый)",
  },
  "settings.clear": { en: "Clear", ru: "Очистить" },
  "settings.clearing": { en: "Clearing…", ru: "Очищаю…" },
  "settings.test": { en: "Test", ru: "Тест" },
  "settings.testing": { en: "Testing…", ru: "Проверяю…" },
  "settings.saveSettings": { en: "Save settings", ru: "Сохранить настройки" },
  "settings.reloadStatus": { en: "Reload status", ru: "Обновить статус" },
  "settings.about": { en: "About", ru: "О программе" },
  "settings.checking": { en: "Checking…", ru: "Проверяю…" },
  "settings.updateAvailable": { en: "Update available: {v}", ru: "Доступно обновление: {v}" },
  "settings.openRelease": { en: "Open release", ru: "Открыть релиз" },
  "settings.upToDate": { en: "Up to date ({v})", ru: "Актуальная версия ({v})" },
  "settings.tuffboxIde": { en: "TuffBox IDE", ru: "TuffBox IDE" },
  "settings.aboutDesc": {
    en: "Developer harness for Minecraft modpacks.",
    ru: "Инструментальная среда для сборок Minecraft.",
  },
  "settings.version": { en: "Version {v}", ru: "Версия {v}" },
  "settings.appIcon": { en: "App icon", ru: "Иконка приложения" },
  "settings.iconHint": {
    en: "Shown on the left rail and on this About page.",
    ru: "Показана на левом рейле и на этой странице.",
  },
  "settings.classic": { en: "Classic", ru: "Классика" },
  "settings.creeperBox": { en: "Creeper box", ru: "Крипер" },

  // ── Library pane: menus, dialogs, helpers, toasts ──
  "library.createNew": { en: "Create new…", ru: "Создать…" },
  "library.importFile": {
    en: "Import file (.mrpack / .zip)",
    ru: "Импорт файла (.mrpack / .zip)",
  },
  "library.importFolder": { en: "Import instance folder", ru: "Импорт папки сборки" },
  "library.importGithubRepo": {
    en: "Import GitHub repository",
    ru: "Импорт репозитория GitHub",
  },
  "library.findInCatalog": { en: "Find in catalog", ru: "Найти в каталоге" },
  "library.instancesFolder": { en: "Instances folder", ru: "Папка сборок" },
  "library.selectedInstance": { en: "Selected instance", ru: "Выбранная сборка" },
  "library.exportPrism": { en: "Export Prism zip", ru: "Экспорт Prism zip" },
  "library.renameEllipsis": { en: "Rename…", ru: "Переименовать…" },
  "library.rename": { en: "Rename", ru: "Переименовать" },
  "library.changeIcon": { en: "Change icon…", ru: "Сменить иконку…" },
  "library.clearIcon": { en: "Clear icon", ru: "Убрать иконку" },
  "library.createShortcut": { en: "Create Shortcut", ru: "Создать ярлык" },
  "library.repair": { en: "Repair", ru: "Починить" },
  "library.copyPath": { en: "Copy path", ru: "Копировать путь" },
  "library.removeFromLibrary": { en: "Remove from library", ru: "Убрать из библиотеки" },
  "library.deleteFromDisk": { en: "Delete from disk", ru: "Удалить с диска" },
  "library.openIde": { en: "Open IDE", ru: "Открыть IDE" },
  "library.preview": { en: "Preview", ru: "Превью" },
  "library.newNameFor": {
    en: "New display name for \"{name}\". The folder name stays unchanged.",
    ru: "Новое имя для «{name}». Имя папки не меняется.",
  },
  "library.copyOf": { en: "Create a copy of \"{name}\"", ru: "Копия «{name}»" },
  "library.githubMsg": {
    en: "Public repo only. Paste owner/repo or a github.com URL. No login needed.",
    ru: "Только публичные репозитории. Вставьте owner/repo или ссылку github.com. Вход не нужен.",
  },
  "library.neverPlayed": { en: "Never played", ru: "Не запускалась" },
  "library.justNow": { en: "Just now", ru: "Только что" },
  "library.minAgo": { en: "{n}m ago", ru: "{n} мин назад" },
  "library.hourAgo": { en: "{n}h ago", ru: "{n} ч назад" },
  "library.dayAgo": { en: "{n}d ago", ru: "{n} дн назад" },
  "library.never": { en: "Never", ru: "Никогда" },
  "library.autoWord": { en: "Auto", ru: "Авто" },
  "library.launching": { en: "Launching…", ru: "Запуск…" },
  "library.toastUpdatePartial": {
    en: "Updated {ok}, failed {failed}: {first}",
    ru: "Обновлено {ok}, ошибок {failed}: {first}",
  },
  "library.toastUpdated1": { en: "Updated 1 mod", ru: "Обновлён 1 мод" },
  "library.toastUpdatedN": { en: "Updated {n} mods", ru: "Обновлено модов: {n}" },
  "library.toastNothingToUpdate": { en: "Nothing to update", ru: "Обновлять нечего" },
  "library.toastUpdateFailed": { en: "Update failed: {e}", ru: "Не удалось обновить: {e}" },
  "library.toastFolderNotSet": {
    en: "Instances folder is not set.",
    ru: "Папка сборок не задана.",
  },
  "library.toastLibraryRefreshed": { en: "Library refreshed", ru: "Библиотека обновлена" },
  "library.toastOversized": {
    en: "This pack is still publishing oversized assets. Try again when the author finishes.",
    ru: "Автор ещё загружает крупные файлы этого пака. Повторите позже.",
  },
  "library.toastSetFolder": {
    en: "Set an instances folder in Settings first.",
    ru: "Сначала задайте папку сборок в настройках.",
  },
  "library.toastImported": { en: "Imported \"{name}\"", ru: "Импортировано: «{name}»" },
  "library.toastExportCopied": {
    en: "Exported .mrpack — path copied: {path}",
    ru: "Экспортировано .mrpack — путь скопирован: {path}",
  },
  "library.toastExport": { en: "Exported .mrpack: {path}", ru: "Экспортировано .mrpack: {path}" },
  "library.toastPrismCopied": {
    en: "Exported Prism zip — path copied: {path}",
    ru: "Экспортирован Prism zip — путь скопирован: {path}",
  },
  "library.toastPrism": {
    en: "Exported Prism zip: {path}",
    ru: "Экспортирован Prism zip: {path}",
  },
  "library.toastServerCopied": {
    en: "Exported server pack — path copied: {path}",
    ru: "Экспортирован сервер-пак — путь скопирован: {path}",
  },
  "library.toastServer": {
    en: "Exported server pack: {path}",
    ru: "Экспортирован сервер-пак: {path}",
  },
  "library.toastShortcut": {
    en: "Desktop shortcut created — double-click to launch: {path}",
    ru: "Ярлык на рабочем столе создан — дважды кликните для запуска: {path}",
  },
  "library.toastPathCopied": {
    en: "Instance folder path copied",
    ru: "Путь к папке сборки скопирован",
  },
  "library.repairRedownloaded": { en: "{n} re-downloaded", ru: "Перекачано: {n}" },
  "library.repairFailed": { en: "{n} failed", ru: "Ошибок: {n}" },
  "library.repairDupes": { en: "{n} duplicate group(s)", ru: "Групп дублей: {n}" },
  "library.repairWrongLoader": { en: "{n} wrong-loader jar(s)", ru: "Чужой загрузчик: {n}" },
  "library.toastRepairOk": {
    en: "All mod files present and valid.",
    ru: "Все файлы модов на месте и валидны.",
  },
  "library.toastRepairReport": { en: "Repair report: {parts}.", ru: "Отчёт починки: {parts}." },
  "library.toastRepairFindings": {
    en: "Repair finished with findings. {parts}",
    ru: "Починка завершилась с замечаниями. {parts}",
  },
  "library.toastRemoved": {
    en: "Removed \"{name}\" from library",
    ru: "«{name}» убрана из библиотеки",
  },
  "library.toastDeleted": { en: "Deleted \"{name}\"", ru: "«{name}» удалена" },
  "library.toastIconUpdated": {
    en: "Icon updated for \"{name}\"",
    ru: "Иконка «{name}» обновлена",
  },
  "library.toastIconCleared": {
    en: "Icon cleared for \"{name}\"",
    ru: "Иконка «{name}» убрана",
  },
  "library.toastRenamed": { en: "Renamed to \"{name}\"", ru: "Переименовано: «{name}»" },
  "library.toastCopiedTo": { en: "Copied to: {path}", ru: "Скопировано: {path}" },

  // ── Library tabs / import ──
  "library.yourPacks": { en: "Your packs", ru: "Ваши сборки" },
  "library.discover": { en: "Discover", ru: "Обзор" },
  "library.createTab": { en: "Create", ru: "Создать" },
  "library.createNewTitle": {
    en: "Create a new instance",
    ru: "Создать новую сборку",
  },
  "library.importBtn": { en: "Import", ru: "Импорт" },
  "library.importing": { en: "Importing…", ru: "Импорт…" },
  "library.importTitle": {
    en: "Import .mrpack, .zip, or Prism/MultiMC/CurseForge instance",
    ru: "Импорт .mrpack, .zip или сборки Prism/MultiMC/CurseForge",
  },
  "library.importSources": { en: "Import sources", ru: "Источники импорта" },
  "library.importFileShort": { en: "File (.mrpack / .zip)", ru: "Файл (.mrpack / .zip)" },
  "library.importFolderShort": { en: "Instance folder", ru: "Папка сборки" },
  "library.importGithubShort": { en: "GitHub repository", ru: "Репозиторий GitHub" },
  "library.sections": { en: "Library sections", ru: "Разделы библиотеки" },

  // ── Discover (catalog) ──
  "discover.provider": { en: "Catalog provider", ru: "Каталог" },
  "discover.both": { en: "Both", ru: "Оба" },
  "discover.bothTitle": {
    en: "Search both catalogs at once",
    ru: "Искать в обоих каталогах сразу",
  },
  "discover.searchAria": { en: "Search modpacks", ru: "Поиск сборок" },
  "discover.search": { en: "Search", ru: "Искать" },
  "discover.phModrinth": { en: "Search Modrinth modpacks…", ru: "Поиск на Modrinth…" },
  "discover.phCurseForge": { en: "Search CurseForge modpacks…", ru: "Поиск на CurseForge…" },
  "discover.phBoth": { en: "Search modpacks…", ru: "Поиск сборок…" },
  "discover.searching": { en: "Searching catalogs…", ru: "Ищу в каталогах…" },
  "discover.packsCount": { en: "{n} packs", ru: "Сборок: {n}" },
  "discover.forQuery": { en: "for “{q}”", ru: "по запросу «{q}»" },
  "discover.loading": { en: "Loading modpacks…", ru: "Загружаю сборки…" },
  "discover.none": { en: "No packs found", ru: "Ничего не найдено" },
  "discover.noMatch": { en: "Nothing matches “{q}”", ru: "По запросу «{q}» ничего нет" },
  "discover.tryOther": {
    en: "Try a different search",
    ru: "Попробуйте другой запрос",
  },
  "discover.inProvider": { en: "in {p}", ru: "в {p}" },
  "discover.unknownAuthor": { en: "Unknown author", ru: "Автор неизвестен" },
  "discover.page": { en: "Page", ru: "Страница" },
  "discover.openPageTitle": {
    en: "Open catalog page in TuffBox",
    ru: "Открыть страницу в TuffBox",
  },
  "discover.add": { en: "Add to TuffBox", ru: "Добавить в TuffBox" },
  "discover.adding": { en: "Adding…", ru: "Добавляю…" },
  "discover.downloadTo": { en: "Download to", ru: "Папка загрузок" },
  "discover.unsaved": { en: "· unsaved", ru: "· не сохранено" },
  "discover.folderPh": {
    en: "Choose a folder for modpacks",
    ru: "Выберите папку для сборок",
  },
  "discover.browseTitle": { en: "Browse", ru: "Обзор" },
  "discover.toastNothingImportable": {
    en: "Nothing importable in the dropped selection.",
    ru: "В выбранных файлах нет того, что можно импортировать.",
  },
  "discover.toastCreated": { en: "Created \"{name}\"", ru: "Создано: «{name}»" },
  "discover.toastDlSaved": {
    en: "Download folder saved.",
    ru: "Папка загрузок сохранена.",
  },
  "discover.toastPickFolder": {
    en: "Pick a download folder first.",
    ru: "Сначала выберите папку загрузок.",
  },
  "discover.toastNoPage": {
    en: "No catalog page for this modpack.",
    ru: "У этой сборки нет страницы в каталоге.",
  },
  "discover.toastOpenFail": {
    en: "Could not open link: {e}",
    ru: "Не удалось открыть ссылку: {e}",
  },
  "discover.toastResolving": {
    en: "Resolving CurseForge files for {name}…",
    ru: "Получаю файлы CurseForge для {name}…",
  },
  "discover.toastDownloading": {
    en: "Downloading {name}…",
    ru: "Скачиваю {name}…",
  },
  "discover.toastAdded": {
    en: "Added \"{name}\" to {dir}.",
    ru: "«{name}» добавлена в {dir}.",
  },
  "discover.toastAddFail": {
    en: "Could not add {name}: {e}",
    ru: "Не удалось добавить {name}: {e}",
  },
  "discover.toastLinkRejected": {
    en: "Install link rejected: \"{raw}\" is not a GitHub owner/repo.",
    ru: "Ссылка отклонена: «{raw}» — это не GitHub owner/repo.",
  },
  "discover.importNoPath": {
    en: "Import returned no path",
    ru: "Импорт не вернул путь",
  },
  "discover.importFallbackName": { en: "Imported pack", ru: "Импортированный пак" },

  // ── Create tab ──
  "create.startPack": { en: "Start a pack", ru: "Начните сборку" },
  "create.startHint": {
    en: "Blank instance, import a pack file, or browse Modrinth / CurseForge in Discover.",
    ru: "Чистая сборка, импорт файла или Modrinth / CurseForge в разделе «Обзор».",
  },
  "create.createModpack": { en: "Create modpack", ru: "Создать сборку" },
  "create.blankHint": {
    en: "Blank · Fabric / Forge / NeoForge / Quilt",
    ru: "Чистая · Fabric / Forge / NeoForge / Quilt",
  },
  "create.importPack": { en: "Import pack", ru: "Импортировать пак" },
  "create.importHint": {
    en: ".mrpack · zip · Prism · MultiMC · CurseForge",
    ru: ".mrpack · zip · Prism · MultiMC · CurseForge",
  },
  "create.browsePacks": { en: "Browse packs", ru: "Каталог сборок" },
  "create.browseHint": {
    en: "Modrinth · CurseForge — Library Discover",
    ru: "Modrinth · CurseForge — «Обзор» библиотеки",
  },

  // ── Drop overlay ──
  "drop.import": { en: "Drop to import", ru: "Отпустите для импорта" },
  "drop.copying": { en: "Copying dropped files…", ru: "Копирую файлы…" },

  // ── Instance content (servers) ──
  "content.title": { en: "Instance content", ru: "Содержимое сборки" },
  "content.browseCatalog": { en: "Browse the catalog", ru: "Открыть каталог" },
  "content.browseCatalogTitle": {
    en: "Browse and install mods in a separate window",
    ru: "Открыть и установить моды в отдельном окне",
  },
  "content.noServers": {
    en: "No servers yet — add one above to track its status.",
    ru: "Серверов пока нет — добавьте один выше, чтобы следить за статусом.",
  },
  "content.offline": { en: "offline", ru: "офлайн" },
  "content.serverName": { en: "Server name", ru: "Имя сервера" },
  "content.serverAddr": { en: "Server address", ru: "Адрес сервера" },
  "content.namePh": { en: "Name", ru: "Имя" },
  "content.join": { en: "Join server", ru: "На сервер" },
  "content.toastAdded": { en: "Server added", ru: "Сервер добавлен" },

  // ── Manager toasts ──
  "manager.toastBatchFail": {
    en: "{label}: {n} failed — {first}",
    ru: "{label}: ошибок {n} — {first}",
  },
  "manager.toastBatchDone": { en: "{label}: {n} done", ru: "{label}: готово {n}" },
  "manager.toastReadFail": {
    en: "Failed to read mods: {e}",
    ru: "Не удалось прочитать моды: {e}",
  },
  "manager.toastAllUpToDate": {
    en: "All mods are up to date",
    ru: "Все моды актуальны",
  },
  "manager.toastCheckFail": {
    en: "Update check failed: {e}",
    ru: "Проверка обновлений не удалась: {e}",
  },
  "manager.toastModUpdated": { en: "{name} updated", ru: "{name} обновлён" },
  "manager.toastModRemoved": { en: "{name} removed", ru: "{name} убран" },
  "manager.toastRemoveFail": { en: "Remove failed: {e}", ru: "Не удалось убрать: {e}" },
  "manager.toastNoVersions": {
    en: "No alternative versions found for this mod",
    ru: "Альтернативных версий у этого мода нет",
  },
  "manager.toastVersionLookupFail": {
    en: "Version lookup failed: {e}",
    ru: "Не удалось получить версии: {e}",
  },
  "manager.toastVersionFail": {
    en: "Version change failed: {e}",
    ru: "Не удалось сменить версию: {e}",
  },
  "manager.toastSynced": {
    en: "Mods folder synced ({n} entries)",
    ru: "Папка модов синхронизирована ({n})",
  },
  "manager.toastBackupListFail": {
    en: "Failed to list backups: {e}",
    ru: "Не удалось получить список бэкапов: {e}",
  },
  "manager.toastBackupCreated": {
    en: "Backup created: {name}",
    ru: "Бэкап создан: {name}",
  },
  "manager.toastBackupFail": { en: "Backup failed: {e}", ru: "Бэкап не удался: {e}" },
  "manager.toastRestored": { en: "Restored \"{name}\"", ru: "Восстановлено: «{name}»" },
  "manager.toastRestoreFail": {
    en: "Restore failed: {e}",
    ru: "Восстановление не удалось: {e}",
  },
  "manager.toastBackupDeleted": { en: "Backup deleted", ru: "Бэкап удалён" },
  "manager.toastDeleteFail": {
    en: "Delete failed: {e}",
    ru: "Удаление не удалось: {e}",
  },
  "manager.toastSavesFail": {
    en: "Failed to read saves: {e}",
    ru: "Не удалось прочитать сохранения: {e}",
  },
  "manager.toastStopFirst": {
    en: "Stop the instance before joining a world.",
    ru: "Остановите сборку перед входом в мир.",
  },
  "manager.toastLaunchFail": { en: "Launch failed: {e}", ru: "Запуск не удался: {e}" },
  "manager.toastWorldBackedUp": {
    en: "World backed up: {file}",
    ru: "Мир заархивирован: {file}",
  },
  "manager.toastWorldDeleted": {
    en: "World \"{name}\" deleted (backup kept)",
    ru: "Мир «{name}» удалён (бэкап сохранён)",
  },
  "manager.toastShotsFail": {
    en: "Failed to list screenshots: {e}",
    ru: "Не удалось получить скриншоты: {e}",
  },
  "manager.toastShotDeleted": { en: "Screenshot deleted", ru: "Скриншот удалён" },
  "manager.toastHealthFail": {
    en: "Health check failed: {e}",
    ru: "Проверка состояния не удалась: {e}",
  },
  "manager.toastKept": { en: "Kept {name}", ru: "Оставлен {name}" },
  "manager.toastValidationFail": {
    en: "Validation failed: {e}",
    ru: "Проверка не удалась: {e}",
  },
};

/** Translate `key` for the active locale, filling `{token}` placeholders. */
export const t = derived(locale, (l) => {
  return (key: string, params?: Record<string, string | number>): string => {
    const entry = dict[key];
    const raw = entry ? entry[l] : key;
    if (!params) return raw;
    return raw.replace(/\{(\w+)\}/g, (m, name: string) =>
      name in params ? String(params[name]) : m,
    );
  };
});

export type Translate = typeof t extends import("svelte/store").Readable<infer F> ? F : never;
