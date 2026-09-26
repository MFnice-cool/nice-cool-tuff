/**
 * Browser preview mock for TuffBox IDE.
 *
 * When the frontend is served via `vite dev` without Tauri (browser preview
 * on e2b.app), there is no Rust backend.  This module fakes every
 * `invoke()` so every tab renders with realistic test data instead of
 * an empty "Desktop IPC unavailable" error.
 *
 * Entry-point: `main.ts` calls `installBrowserMockIfNeeded()` before mounting.
 * Nothing happens when running inside Tauri.
 */

import { mockIPC, mockWindows, mockConvertFileSrc } from "@tauri-apps/api/mocks";
import { isTauri } from "@tauri-apps/api/core";

// ---------------------------------------------------------------------------
// 1.  Chosen modpack — exposed for the preview UI
// ---------------------------------------------------------------------------

/** Human choice for the preview session — all mock data revolves around it. */
export const CHOSEN_MODPACK = {
  id: "create-aeronautics",
  name: "Create: Aeronautics",
  version: "0.4.2",
  mcVersion: "1.20.1",
  loader: "fabric" as const,
  loaderVersion: "0.15.7",
  description:
    "Полёты на блоках Create — дирижабли, воздушные корабли и автоматика в Fabric 1.20.1.",
  path: "/home/user/TuffBox/instances/Create-Aeronautics/.tuffbox.json",
} as const;

// ---------------------------------------------------------------------------
// 2.  Realistic aggregate data
// ---------------------------------------------------------------------------

const MOCK_PROJECTS = [
  {
    path: CHOSEN_MODPACK.path,
    info: {
      id: CHOSEN_MODPACK.id,
      name: CHOSEN_MODPACK.name,
      version: CHOSEN_MODPACK.version,
      minecraftVersion: CHOSEN_MODPACK.mcVersion,
      loaderKind: CHOSEN_MODPACK.loader,
      loaderVersion: CHOSEN_MODPACK.loaderVersion,
      javaPath: null,
      memoryMb: 4096,
      jvmArgs: ["-XX:+UseG1GC"],
      playerName: "Aviator",
    },
  },
  {
    path: "/home/user/TuffBox/instances/TuffCraft-RPG/.tuffbox.json",
    info: {
      id: "tuffcraft-rpg",
      name: "TuffCraft RPG",
      version: "1.9.3",
      minecraftVersion: "1.20.1",
      loaderKind: "forge",
      loaderVersion: "47.1.3",
      javaPath: null,
      memoryMb: 6144,
      jvmArgs: ["-XX:+UseG1GC", "-XX:+UnlockExperimentalVMOptions"],
      playerName: "Hero",
    },
  },
  {
    path: "/home/user/TuffBox/instances/ATM9/.tuffbox.json",
    info: {
      id: "atm9",
      name: "All The Mods 9",
      version: "0.2.61",
      minecraftVersion: "1.20.1",
      loaderKind: "neoforge",
      loaderVersion: "47.1.106",
      javaPath: "/usr/lib/jvm/java-17-openjdk",
      memoryMb: 8192,
      jvmArgs: ["-XX:+UseG1GC"],
      playerName: "Explorer",
    },
  },
] as const;

const MOCK_MODS: Record<string, any[]> = {
  [CHOSEN_MODPACK.path]: [
    {
      id: "create",
      name: "Create",
      version: "0.5.1-f",
      side: "both",
      source: "modrinth",
      projectId: "Xbc0Y",
      fileName: "create-1.20.1-0.5.1.f.jar",
      iconUrl: "https://cdn.modrinth.com/data/Xbc0Y/icon.png",
      clientSide: "required",
      serverSide: "required",
      contentType: "mod",
      disabled: false,
      status: ["ok"],
    },
    {
      id: "create-aeronautics",
      name: "Create: Aeronautics",
      version: "0.1.1",
      side: "both",
      source: "modrinth",
      projectId: "aero123",
      fileName: "create-aeronautics-0.1.1.jar",
      iconUrl: null,
      clientSide: "required",
      serverSide: "required",
      contentType: "mod",
      disabled: false,
      status: ["ok"],
    },
    {
      id: "jei",
      name: "Just Enough Items",
      version: "15.2.0.27",
      side: "client",
      source: "modrinth",
      projectId: "u6dRKp63",
      fileName: "jei-1.20.1-15.2.0.27.jar",
      iconUrl: "https://cdn.modrinth.com/data/u6dRKp63/icon.png",
      clientSide: "required",
      serverSide: "optional",
      contentType: "mod",
      disabled: false,
      status: ["ok"],
    },
    {
      id: "sodium",
      name: "Sodium",
      version: "0.5.8",
      side: "client",
      source: "modrinth",
      projectId: "AANobbMI",
      fileName: "sodium-fabric-0.5.8+mc1.20.1.jar",
      iconUrl: "https://cdn.modrinth.com/data/AANobbMI/icon.png",
      clientSide: "required",
      serverSide: "unsupported",
      contentType: "mod",
      disabled: false,
      status: ["ok"],
    },
    {
      id: "iris",
      name: "Iris Shaders",
      version: "1.6.17",
      side: "client",
      source: "modrinth",
      projectId: "YL57xq9U",
      fileName: "iris-1.6.17+mc1.20.1.jar",
      iconUrl: null,
      clientSide: "required",
      serverSide: "unsupported",
      contentType: "mod",
      disabled: false,
      status: ["ok"],
    },
    {
      id: "ferritecore",
      name: "FerriteCore",
      version: "6.0.1",
      side: "both",
      source: "modrinth",
      projectId: "uXXizFIs",
      fileName: "ferritecore-6.0.1-fabric.jar",
      iconUrl: null,
      clientSide: "required",
      serverSide: "required",
      contentType: "mod",
      disabled: false,
      status: ["ok"],
    },
    {
      id: "architectury",
      name: "Architectury API",
      version: "9.2.14",
      side: "both",
      source: "modrinth",
      projectId: "ljWoqhgK",
      fileName: "architectury-9.2.14-fabric.jar",
      iconUrl: null,
      clientSide: "required",
      serverSide: "required",
      contentType: "mod",
      disabled: false,
      status: ["ok"],
    },
    {
      id: "cloth-config",
      name: "Cloth Config API",
      version: "11.1.118",
      side: "both",
      source: "modrinth",
      projectId: "9s6osm5g",
      fileName: "cloth-config-11.1.118-fabric.jar",
      iconUrl: null,
      clientSide: "required",
      serverSide: "required",
      contentType: "mod",
      disabled: false,
      status: ["ok"],
    },
    {
      id: "embeddium_dup",
      name: "Embeddium (duplicate test)",
      version: "0.3.31",
      side: "client",
      source: "modrinth",
      projectId: "sk9rgfiA",
      fileName: "embeddium-0.3.31+mc1.20.1.jar",
      iconUrl: null,
      clientSide: "required",
      serverSide: "unsupported",
      contentType: "mod",
      disabled: false,
      status: ["duplicate"],
    },
  ],
  "/home/user/TuffBox/instances/TuffCraft-RPG/.tuffbox.json": [
    {
      id: "oculus",
      name: "Oculus",
      version: "1.6.9",
      side: "client",
      source: "modrinth",
      projectId: "GchcoXML",
      fileName: "oculus-mc1.20.1-1.6.9.jar",
      iconUrl: null,
      clientSide: "required",
      serverSide: "unsupported",
      contentType: "mod",
      disabled: false,
      status: ["ok"],
    },
    {
      id: "embeddium",
      name: "Embeddium",
      version: "0.3.31",
      side: "client",
      source: "modrinth",
      projectId: "sk9rgfiA",
      fileName: "embeddium-0.3.31+mc1.20.1.jar",
      iconUrl: null,
      clientSide: "required",
      serverSide: "unsupported",
      contentType: "mod",
      disabled: false,
      status: ["ok"],
    },
  ],
  "/home/user/TuffBox/instances/ATM9/.tuffbox.json": [
    {
      id: "ae2",
      name: "Applied Energistics 2",
      version: "15.0.13",
      side: "both",
      source: "modrinth",
      projectId: "XxWD5pD3",
      fileName: "appliedenergistics2-15.0.13.jar",
      iconUrl: null,
      clientSide: "required",
      serverSide: "required",
      contentType: "mod",
      disabled: false,
      status: ["ok"],
    },
  ],
};

const MOCK_GRAPH: Record<string, any> = {
  [CHOSEN_MODPACK.path]: {
    nodes: [
      { id: "minecraft:1.20.1", kind: "MinecraftVersion", label: "Minecraft 1.20.1", version: "1.20.1", side: "both", metadata: {} },
      { id: "loader:fabric:0.15.7", kind: "Loader", label: "Fabric 0.15.7", version: "0.15.7", side: "both", metadata: {} },
      { id: "mod:create", kind: "Mod", label: "Create 0.5.1-f", version: "0.5.1-f", side: "both", metadata: {} },
      { id: "mod:create-aeronautics", kind: "Mod", label: "Create: Aeronautics 0.1.1", version: "0.1.1", side: "both", metadata: {} },
      { id: "mod:jei", kind: "Mod", label: "JEI 15.2.0.27", version: "15.2.0.27", side: "client", metadata: {} },
      { id: "mod:sodium", kind: "Mod", label: "Sodium 0.5.8", version: "0.5.8", side: "client", metadata: {} },
      { id: "mod:iris", kind: "Mod", label: "Iris 1.6.17", version: "1.6.17", side: "client", metadata: {} },
      { id: "mod:ferritecore", kind: "Mod", label: "FerriteCore 6.0.1", version: "6.0.1", side: "both", metadata: {} },
      { id: "mod:embeddium_dup", kind: "Mod", label: "Embeddium (дубликат)", version: "0.3.31", side: "client", metadata: {} },
    ],
    edges: [
      { from: "mod:create-aeronautics", to: "mod:create", kind: "requires", constraint: ">=0.5.0", reason: "Базовый мод Create" },
      { from: "mod:create", to: "loader:fabric:0.15.7", kind: "requires_loader", constraint: null, reason: null },
      { from: "mod:create", to: "minecraft:1.20.1", kind: "requires_minecraft", constraint: "1.20.1", reason: null },
      { from: "mod:iris", to: "mod:sodium", kind: "requires", constraint: ">=0.5.0", reason: "Iris требует Sodium" },
      { from: "mod:embeddium_dup", to: "mod:sodium", kind: "conflicts", constraint: null, reason: "Обе заменяют рендер" },
    ],
    source: "local",
    generatedAt: new Date().toISOString(),
  },
};

const MOCK_CONFIGS = [
  { path: "config/create-common.toml", name: "create-common.toml", extension: "toml", size: 8240, modified: Date.now() / 1000 - 3600 },
  { path: "config/jei/jei.toml", name: "jei.toml", extension: "toml", size: 1200, modified: Date.now() / 1000 - 7200 },
  { path: "config/sodium-options.json", name: "sodium-options.json", extension: "json", size: 2400, modified: Date.now() / 1000 - 1800 },
  { path: "config/create-client.toml", name: "create-client.toml", extension: "toml", size: 3200, modified: Date.now() / 1000 - 900 },
];

const MOCK_WORLDS = [
  {
    name: "Aeronautics Test World",
    size: 48234,
    sizeFormatted: "48 MB",
    hasLevelDat: true,
    hasIcon: true,
    displayName: "Aeronautics Test World",
    gameType: "survival",
    difficulty: "normal",
    hardcore: false,
    cheatsEnabled: true,
    lastPlayed: Date.now() - 1000 * 60 * 30,
  },
  {
    name: "Skyblock Create",
    size: 12400,
    sizeFormatted: "12 MB",
    hasLevelDat: true,
    hasIcon: false,
    displayName: "Skyblock Create",
    gameType: "survival",
    difficulty: "hard",
    hardcore: false,
    cheatsEnabled: false,
    lastPlayed: Date.now() - 1000 * 60 * 60 * 24 * 2,
  },
];

const MOCK_QUEST_BOOK = {
  chapters: [
    {
      id: "intro",
      title: "Введение в Create",
      icon: "create:cogwheel",
      quests: [
        {
          id: "quest_kinetic",
          title: "Кинетическая энергия",
          subtitle: "Собери водяное колесо",
          description: ["Поставь водяное колесо рядом с рекой и подключи к валу."],
          x: 0,
          y: 0,
          icon: "create:water_wheel",
          dependencies: [],
          tasks: [{ id: "task_water_wheel", type: "item", title: "Скрафти водяное колесо", value: "create:water_wheel" }],
          rewards: [{ id: "reward_cog", type: "item", title: "Шестерня", properties: { item: "create:cogwheel", count: 4 } }],
          optional: false,
        },
        {
          id: "quest_airship",
          title: "Первый дирижабль",
          subtitle: "Собери корпус из рамок",
          description: ["Используй клей и рамки Create: Aeronautics чтобы собрать летающий корабль."],
          x: 2,
          y: 0,
          icon: "create_aeronautics:airship_frame",
          dependencies: ["quest_kinetic"],
          tasks: [{ id: "task_airship", type: "item", title: "Собери корпус", value: "create_aeronautics:airship_frame*16" }],
          rewards: [{ id: "reward_balloon", type: "item", title: "Воздушный шар", properties: { item: "create_aeronautics:balloon", count: 8 } }],
          optional: false,
        },
      ],
      group: null,
      orderIndex: 0,
      filename: "intro.snbt",
      extras: {},
      sourceFile: "config/ftbquests/quests/chapters/intro.snbt",
    },
    {
      id: "automation",
      title: "Автоматизация",
      icon: "create:mechanical_press",
      quests: [
        {
          id: "quest_press",
          title: "Механический пресс",
          subtitle: null,
          description: ["Автоматизируй крафт листов."],
          x: 0,
          y: 1,
          icon: "create:mechanical_press",
          dependencies: ["quest_kinetic"],
          tasks: [{ id: "task_press", type: "item", title: "Поставь пресс", value: "create:mechanical_press" }],
          rewards: [{ id: "reward_iron", type: "item", title: "Железный лист", properties: { item: "create:iron_sheet", count: 3 } }],
          optional: false,
        },
      ],
      group: null,
      orderIndex: 1,
      filename: "automation.snbt",
      extras: {},
      sourceFile: "config/ftbquests/quests/chapters/automation.snbt",
    },
  ],
  title: "Create: Aeronautics — Квесты",
  subtitle: "Научись летать и автоматизировать",
  chapterGroups: [{ id: "main", title: "Основные" }],
  rewardTables: [],
  bookSettings: {},
  locales: {},
  activeLocale: null,
  loadWarnings: [],
};

const MOCK_SNAPSHOTS = [
  {
    id: "snap_20250922_070000",
    name: "Перед добавлением Aeronautics",
    createdAt: new Date(Date.now() - 1000 * 60 * 60 * 2).toISOString(),
    reason: "manual",
    manifestPath: CHOSEN_MODPACK.path,
    lockfilePath: null,
    changedFiles: ["mods/create-aeronautics-0.1.1.jar"],
    sizeBytes: 12345678,
    tags: ["preflight"],
    crashFingerprintKey: null,
    reportId: null,
    planSource: null,
    matchedCaseIds: [],
    operation: "add_mod",
    actionsSummary: ["Добавлен Create: Aeronautics 0.1.1"],
    actor: "user",
  },
  {
    id: "snap_20250920_120000",
    name: "Автоснапшот — оптимизация",
    createdAt: new Date(Date.now() - 1000 * 60 * 60 * 24 * 2).toISOString(),
    reason: "auto",
    manifestPath: CHOSEN_MODPACK.path,
    lockfilePath: null,
    changedFiles: ["mods/sodium-fabric-0.5.8+mc1.20.1.jar", "mods/ferritecore-6.0.1-fabric.jar"],
    sizeBytes: 4567890,
    tags: ["auto"],
    crashFingerprintKey: null,
    reportId: null,
    planSource: null,
    matchedCaseIds: [],
    operation: "optimize",
    actionsSummary: ["Установлены Sodium, FerriteCore"],
    actor: "system",
  },
];

const MOCK_SEARCH_RESULTS = [
  {
    id: "create",
    slug: "create",
    name: "Create",
    description: "Технический мод про шестерни, конвейеры и автоматизацию — основа для Aeronautics.",
    projectType: "mod",
    iconUrl: "https://cdn.modrinth.com/data/Xbc0Y/icon.png",
    clientSide: "required",
    serverSide: "required",
    author: "simibubi",
    downloads: 45231000,
    follows: 12000,
    dateModified: "2024-09-01T00:00:00Z",
    categories: ["technology", "utility"],
  },
  {
    id: "sodium",
    slug: "sodium",
    name: "Sodium",
    description: "Оптимизация рендера — +200% FPS на Fabric.",
    projectType: "mod",
    iconUrl: "https://cdn.modrinth.com/data/AANobbMI/icon.png",
    clientSide: "required",
    serverSide: "unsupported",
    author: "jellysquid",
    downloads: 32000000,
    follows: 9000,
    dateModified: "2024-08-15T00:00:00Z",
    categories: ["optimization"],
  },
  {
    id: "jei",
    slug: "jei",
    name: "Just Enough Items",
    description: "Просмотр рецептов и предметов.",
    projectType: "mod",
    iconUrl: "https://cdn.modrinth.com/data/u6dRKp63/icon.png",
    clientSide: "required",
    serverSide: "optional",
    author: "mezz",
    downloads: 28000000,
    follows: 7000,
    dateModified: "2024-07-20T00:00:00Z",
    categories: ["utility"],
  },
  {
    id: "iris",
    slug: "iris",
    name: "Iris Shaders",
    description: "Шейдеры для Sodium.",
    projectType: "mod",
    iconUrl: null,
    clientSide: "required",
    serverSide: "unsupported",
    author: "IMS",
    downloads: 18000000,
    follows: 5000,
    dateModified: "2024-06-10T00:00:00Z",
    categories: ["optimization"],
  },
  {
    id: "ferritecore",
    slug: "ferritecore",
    name: "FerriteCore",
    description: "Сжимает память — меньше RAM на больших сборках.",
    projectType: "mod",
    iconUrl: null,
    clientSide: "required",
    serverSide: "required",
    author: "malte0811",
    downloads: 12000000,
    follows: 3000,
    dateModified: "2024-05-01T00:00:00Z",
    categories: ["optimization"],
  },
  {
    id: "create-aeronautics",
    slug: "create-aeronautics",
    name: "Create: Aeronautics",
    description: "Дирижабли и воздушные корабли для Create.",
    projectType: "mod",
    iconUrl: null,
    clientSide: "required",
    serverSide: "required",
    author: "Create Aeronautics Team",
    downloads: 45000,
    follows: 800,
    dateModified: "2024-09-10T00:00:00Z",
    categories: ["technology", "transportation"],
  },
];

function getModsForPath(path: string) {
  return MOCK_MODS[path] ?? MOCK_MODS[CHOSEN_MODPACK.path] ?? [];
}

function getGraphForPath(path: string) {
  return MOCK_GRAPH[path] ?? MOCK_GRAPH[CHOSEN_MODPACK.path] ?? { nodes: [], edges: [], source: "local", generatedAt: new Date().toISOString() };
}

// ---------------------------------------------------------------------------
// 3.  Install mock
// ---------------------------------------------------------------------------

let installed = false;

export async function installBrowserMockIfNeeded(): Promise<boolean> {
  // Already inside Tauri — nothing to mock.
  if (typeof window !== "undefined" && (window as any).isTauri) {
    try {
      if (isTauri()) return false;
    } catch {
      // fall through to mock
    }
  }

  if (installed) return true;
  installed = true;

  // Must run before any invoke/listen.
  mockWindows("main", "mods-browser");
  mockConvertFileSrc("linux");
  (window as any).isTauri = true;
  // Some Tauri checks look at globalThis.isTauri — mirror it.
  try {
    (globalThis as any).isTauri = true;
  } catch {}

  // Seed localStorage so Dashboard's recentProjects store has something
  // even before the first home bootstrap event.
  try {
    const existing = localStorage.getItem("recentProjects");
    if (!existing) {
      const seed = MOCK_PROJECTS.map((p) => ({ path: p.path, info: p.info }));
      localStorage.setItem("recentProjects", JSON.stringify(seed));
    }
  } catch {}

  mockIPC(async (cmd: string, args: any) => {
    // Helpful during development — uncomment to see what's being mocked:
    // console.debug(`[mockIPC] ${cmd}`, args);

    switch (cmd) {
      // --- Home bootstrap ---
      case "get_home_bootstrap": {
        return {
          recent: MOCK_PROJECTS.map((p) => ({ path: p.path, info: p.info })),
          lastOpened: CHOSEN_MODPACK.path,
          launcherSettings: {
            theme: "tuffbox-dark",
            potatoPc: false,
            perfAutoDetected: true,
            concurrentDownloads: 4,
            gameResolution: null,
            preLaunchHook: null,
            postExitHook: null,
            wrapperCommand: null,
            runtimePath: null,
            instancesPath: null,
            defaultJavaPath: null,
            javaCustomArgs: null,
            defaultMemoryMb: 4096,
            youtubeInlinePlayer: true,
            showYoutubeOnHome: false,
            newsShowUpdates: true,
            ingameOverlay: false,
            cpuAffinityMode: "off",
            cpuAffinityMask: null,
            gpuPreference: "auto",
            autoHideWorkflowRail: false,
            hideIdeNextBar: false,
            sidebarMode: "full",
            uiScalePercent: 100,
            uiScaleMode: "auto",
            roundedCorners: true,
            homeBackdrop: true,
          },
          auth: {
            loggedIn: true,
            profile: {
              uuid: "mock-uuid-aviator",
              name: "Aviator",
              skinUrl: null,
              capeUrl: null,
              capes: [],
            },
            expiresAt: Date.now() + 1000 * 60 * 60 * 24,
            loginType: "offline",
            skinSource: "mojang",
            capeProvider: "mojang",
            accounts: [
              { uuid: "mock-uuid-aviator", name: "Aviator", loginType: "offline", skinSource: "mojang", addedAt: Date.now() - 100000 },
              { uuid: "mock-uuid-hero", name: "Hero", loginType: "microsoft", skinSource: "mojang", addedAt: Date.now() - 200000 },
            ],
            activeAccountUuid: "mock-uuid-aviator",
          },
          skinPaths: {},
          running: [],
          statsByPath: {
            [CHOSEN_MODPACK.path]: { playtime: 7320, lastLaunch: new Date(Date.now() - 1000 * 60 * 60 * 5).toISOString() },
            "/home/user/TuffBox/instances/TuffCraft-RPG/.tuffbox.json": { playtime: 15400, lastLaunch: new Date(Date.now() - 1000 * 60 * 60 * 24).toISOString() },
            "/home/user/TuffBox/instances/ATM9/.tuffbox.json": { playtime: 30200, lastLaunch: new Date(Date.now() - 1000 * 60 * 60 * 48).toISOString() },
          },
          iconsByPath: {},
          sizesByPath: {
            [CHOSEN_MODPACK.path]: "1.2 GB",
            "/home/user/TuffBox/instances/TuffCraft-RPG/.tuffbox.json": "892 MB",
            "/home/user/TuffBox/instances/ATM9/.tuffbox.json": "2.4 GB",
          },
          selectedSummary: {
            id: CHOSEN_MODPACK.id,
            name: CHOSEN_MODPACK.name,
            version: CHOSEN_MODPACK.version,
            minecraftVersion: CHOSEN_MODPACK.mcVersion,
            loaderKind: CHOSEN_MODPACK.loader,
            loaderVersion: CHOSEN_MODPACK.loaderVersion,
            javaPath: null,
            memoryMb: 4096,
            jvmArgs: ["-XX:+UseG1GC"],
            playerName: "Aviator",
            manifestPath: CHOSEN_MODPACK.path,
          },
        };
      }
      case "get_home_project_briefs": {
        const paths: string[] = args?.paths ?? [];
        return paths.map((p: string) => ({
          path: p,
          stats: { playtime: 7320, lastLaunch: new Date().toISOString() },
          sizeLabel: "1.2 GB",
          iconDataUrl: null,
        }));
      }
      case "get_account_skin_paths":
        return {};
      case "invalidate_home_project_cache":
        return null;
      case "load_recent_projects": {
        try {
          const raw = localStorage.getItem("recentProjects");
          if (raw) return JSON.parse(raw);
        } catch {}
        return MOCK_PROJECTS.map((p) => ({ path: p.path, info: p.info }));
      }
      case "save_recent_projects":
        try {
          localStorage.setItem("recentProjects", JSON.stringify(args?.projects ?? []));
        } catch {}
        return null;
      case "get_last_opened_project":
        return CHOSEN_MODPACK.path;
      case "set_last_opened_project":
        return null;
      case "get_project_dir": {
        const p: string = args?.path ?? CHOSEN_MODPACK.path;
        return p.replace(/\.tuffbox\.json$/, "").replace(/\/$/, "");
      }
      case "resolve_project_path":
        return args?.path ?? CHOSEN_MODPACK.path;
      case "validate_project": {
        const p: string = args?.path ?? CHOSEN_MODPACK.path;
        const found = MOCK_PROJECTS.find((x) => x.path === p);
        if (found) {
          return {
            id: found.info.id,
            name: found.info.name,
            version: found.info.version,
            minecraftVersion: found.info.minecraftVersion,
            loaderKind: found.info.loaderKind,
            loaderVersion: found.info.loaderVersion,
            javaPath: found.info.javaPath,
            memoryMb: found.info.memoryMb,
            jvmArgs: found.info.jvmArgs,
            playerName: found.info.playerName,
            manifestPath: found.path,
          };
        }
        // fallback to chosen
        const c = MOCK_PROJECTS[0];
        return {
          id: c.info.id,
          name: c.info.name,
          version: c.info.version,
          minecraftVersion: c.info.minecraftVersion,
          loaderKind: c.info.loaderKind,
          loaderVersion: c.info.loaderVersion,
          javaPath: c.info.javaPath,
          memoryMb: c.info.memoryMb,
          jvmArgs: c.info.jvmArgs,
          playerName: c.info.playerName,
          manifestPath: c.path,
        };
      }

      // --- Project meta ---
      case "get_project_schema_status":
        return { current: "0.1.0", detected: "0.1.0", needsMigration: false, supported: ["0.1.0"] };
      case "migrate_project_schema":
        return { current: "0.1.0", detected: "0.1.0", needsMigration: false, supported: ["0.1.0"] };
      case "get_project_brief":
        return { goal: "Полёты и автоматизация на Create", targetAudience: "технари", gameplayPillars: ["automation", "flight"], constraints: [], releaseTargets: ["modrinth"], notes: "тестовый краткий бриф" };
      case "update_project_brief":
      case "update_project_listing":
      case "update_project_brief_and_listing":
        return null;
      case "get_project_listing":
        return {
          name: CHOSEN_MODPACK.name,
          summary: CHOSEN_MODPACK.description,
          bodyMarkdown: `# ${CHOSEN_MODPACK.name}\n\n${CHOSEN_MODPACK.description}\n\n## Особенности\n\n- Дирижабли на Create\n- Автоматизация\n- Оптимизация Sodium + FerriteCore\n`,
          iconPath: null,
          gallery: [],
          categories: ["technology", "transportation"],
          authors: ["TuffBox Team"],
        };
      case "set_project_listing_icon":
      case "clear_project_listing_icon":
      case "add_listing_gallery_image":
      case "add_listing_gallery_bytes":
      case "remove_listing_gallery_image":
      case "reorder_listing_gallery":
        return {
          name: CHOSEN_MODPACK.name,
          summary: CHOSEN_MODPACK.description,
          bodyMarkdown: `# ${CHOSEN_MODPACK.name}`,
          iconPath: null,
          gallery: [],
          categories: ["technology"],
          authors: ["TuffBox Team"],
        };
      case "read_listing_asset":
        return "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+ip1sAAAAASUVORK5CYII=";
      case "ensure_listing_folder":
        return "/tmp/tuffbox-listing";
      case "get_manifest_schema":
        return { type: "object", properties: {} };
      case "run_project_validation":
        return { valid: true, errors: [], warnings: [] };
      case "get_diagnostics": {
        return [
          { severity: "Warning", code: "DUPLICATE_MOD", message: "Embeddium дублируется — Sodium уже покрывает ту же функцию.", relatedNodes: ["mod:embeddium_dup", "mod:sodium"] },
          { severity: "Info", code: "PERF_SUGGESTION", message: "FerriteCore уже оптимизирует память — дополнительный мем-оптимизатор не нужен.", relatedNodes: ["mod:ferritecore"] },
        ];
      }
      case "get_diagnostic_counts":
        return { errorCount: 0, warningCount: 1, cached: false };
      case "get_health_report":
        return {
          manifestPath: CHOSEN_MODPACK.path,
          diagnostics: [
            { severity: "Warning", code: "DUPLICATE_MOD", message: "Embeddium дублируется", relatedNodes: ["mod:embeddium_dup"] },
          ],
          errorCount: 0,
          warningCount: 1,
          hasCrash: false,
          crashReports: [],
          exportBlockers: [],
          missingFiles: [],
          missingHashes: [],
          missingCount: 0,
          hashMismatchCount: 0,
        };
      case "repair_project":
        return { downloaded: [], failed: [], alreadyPresent: ["create", "sodium"], skipped: [], duplicates: [], wrongLoader: [] };
      case "cleanup_project":
        return {};
      case "list_profiles":
        return [
          { id: "client", name: "Client Full", side: "client", memoryMb: 4096, jvmArgs: ["-XX:+UseG1GC"] },
          { id: "server", name: "Server Pack", side: "server", memoryMb: 4096, jvmArgs: ["-XX:+UseG1GC"] },
        ];

      // --- Mods ---
      case "list_mods": {
        const p: string = args?.path ?? CHOSEN_MODPACK.path;
        return getModsForPath(p);
      }
      case "sync_mods_folder":
        return [];
      case "import_local_content_files":
        return { imported: [], identified: [], skipped: [], baselineUpdated: false };
      case "add_modrinth_mod":
      case "add_modrinth_mod_with_dependencies":
        return [];
      case "add_modrinth_mods_with_dependencies":
        return ["create"];
      case "resolve_install_dependencies":
        return [];
      case "add_curseforge_mod":
      case "add_curseforge_mods_with_dependencies":
        return [];
      case "get_mod_presets":
        return { presets: [] };
      case "save_mod_presets_cmd":
        return null;
      case "resolve_preset_mod":
        return { slug: "create", name: "Create", provider: "modrinth", projectId: "Xbc0Y", reason: "core", risk: "low", alreadyInstalled: true };
      case "install_steam_bridge":
        return { modId: "steam-bridge", fileName: "steam-bridge.jar", tag: "v1.0", mcVersion: "1.20.1", loader: "fabric", matchKind: "exact", repo: "tuffbox/steam-bridge" };
      case "remove_project_mod":
      case "update_project_mod":
      case "change_mod_version":
        return {};
      case "get_mod_versions":
        return [
          { id: "create-0.5.1", version: "0.5.1-f", name: "Create 0.5.1-f", gameVersions: ["1.20.1"], loaders: ["fabric"] },
          { id: "create-0.5.0", version: "0.5.0-f", name: "Create 0.5.0-f", gameVersions: ["1.20.1"], loaders: ["fabric"] },
        ];
      case "check_mod_updates":
        return [];
      case "update_all_mods":
        return { dryRun: args?.dryRun ?? false, count: 0, preview: [], updated: [], errors: [], skipped: [] };
      case "retry_failed_mod_downloads":
        return {};
      case "recommend_mods":
        return [];
      case "list_curated_optimize_packs":
        return { loader: "fabric", minecraftVersion: "1.20.1", available: true, unavailableMessage: null, current: null, entries: [] };
      case "preview_curated_optimize_pack":
        return { pack: { projectId: "Xbc0Y", slug: "create", name: "Create", versionId: "v1", versionNumber: "0.5.1" }, mods: [], configActions: [], warnings: [], minecraftVersion: "1.20.1", loader: "fabric" };
      case "install_curated_optimize_pack":
        return {};
      case "build_optimize_plan":
        return { mode: "curated", mods: [], plan: {}, findings: [], warnings: [], minecraftVersion: "1.20.1", loader: "fabric", curatedAvailable: true, catalogSource: "mock" };
      case "apply_optimize_custom_plan":
      case "install_fo_optimize_pack":
        return {};
      case "preview_fo_optimize_pack":
        return { pack: { projectId: "FabulouslyOptimized", slug: "fabulously-optimized", name: "Fabulously Optimized", versionId: "v1", versionNumber: "5.0", minecraftVersion: "1.20.1", loader: "fabric", modCount: 12 }, mods: [], configActions: [], warnings: [], minecraftVersion: "1.20.1", loader: "fabric" };
      case "disable_project_mod":
        return { id: args?.modId ?? "unknown", disabled: true, fileName: "mock.jar" };
      case "enable_project_mod":
        return { id: args?.modId ?? "unknown", disabled: false, fileName: "mock.jar" };
      case "detect_wrong_loader_mods":
        return [];
      case "disable_wrong_loader_jar":
      case "remove_loose_jar":
      case "keep_one_duplicate_mod_jar":
        return "ok";
      case "detect_duplicate_mod_jars":
        return [
          {
            modId: "sodium",
            keepCandidate: "sodium-fabric-0.5.8+mc1.20.1.jar",
            jars: [
              { fileName: "sodium-fabric-0.5.8+mc1.20.1.jar", modId: "sodium", mtimeMs: Date.now() - 100000, size: 1200000, inManifest: true },
              { fileName: "embeddium-0.3.31+mc1.20.1.jar", modId: "embeddium_dup", mtimeMs: Date.now() - 200000, size: 1100000, inManifest: false },
            ],
          },
        ];
      case "check_mod_compatibility":
        return [];
      case "get_mod_info":
        return null;
      case "compare_modpacks":
        return {};

      case "search_modrinth_mods":
      case "search_curseforge_mods":
      case "search_unified_mods":
      case "search_content": {
        const query: string = (args?.query ?? "").toLowerCase();
        const filtered = query
          ? MOCK_SEARCH_RESULTS.filter((r) => r.name.toLowerCase().includes(query) || r.description.toLowerCase().includes(query) || r.slug.includes(query))
          : MOCK_SEARCH_RESULTS;
        return { results: filtered, total: filtered.length };
      }
      case "install_content_batch":
        return ["mock-mod-id"];
      case "preview_modrinth_install":
      case "preview_curseforge_install":
        return {
          projectId: args?.modId ?? "mock",
          slug: "create",
          name: "Create",
          version: "0.5.1",
          fileName: "create-0.5.1.jar",
          side: "both",
          dependencies: [],
          installedDependencies: [],
          dependents: [],
        };
      case "get_modrinth_project_icon":
        return null;
      case "get_modrinth_project":
        return MOCK_SEARCH_RESULTS[0];
      case "list_modrinth_categories":
        return [
          { name: "technology", projectType: "mod", header: "Technology", icon: "" },
          { name: "optimization", projectType: "mod", header: "Optimization", icon: "" },
        ];
      case "list_curseforge_categories":
        return [
          { id: 424, name: "Technology", parentCategoryId: null },
          { id: 423, name: "Optimization", parentCategoryId: null },
        ];
      case "get_mod_user_state":
        return { favorites: {}, lists: {}, ratings: {} };
      case "set_mod_user_state":
      case "create_mod_list":
      case "delete_mod_list":
      case "rename_mod_list":
      case "add_to_mod_list":
      case "remove_from_mod_list":
        return { favorites: {}, lists: {}, ratings: {} };

      // --- Config ---
      case "list_config_files":
        return MOCK_CONFIGS;
      case "read_config_file": {
        const rel: string = args?.relativePath ?? "config/create-common.toml";
        if (rel.endsWith(".toml")) return `# Mock ${rel}\n[general]\nenabled = true\nmaxHeight = 64\n`;
        if (rel.endsWith(".json")) return JSON.stringify({ enabled: true, maxHeight: 64 }, null, 2);
        return "# mock config\n";
      }
      case "write_config_file":
        return { snapshotId: "snap_mock_write" };
      case "search_in_configs":
        return [];
      case "lint_config":
        return [];
      case "format_toml":
        return args?.content ?? "";
      case "tune_config_advise":
        return {
          plan: {},
          explanation: "Мок-объяснение: конфиги в порядке.",
          researchLog: [],
          unknownKeys: [],
          diffs: [],
          validationOk: true,
          validationErrors: [],
          validationWarnings: [],
        };
      case "tune_config_preview_diffs":
        return [];
      case "list_tune_chat_sessions":
        return { sessions: [], corruptSkipped: 0 };
      case "new_tune_chat_session":
        return { id: "tune_mock_1", title: args?.title ?? "Новый чат", messages: [], updatedAt: new Date().toISOString() };
      case "load_tune_chat_session":
      case "save_tune_chat_session":
        return { id: args?.chatId ?? "tune_mock_1", title: "Mock Tune Chat", messages: [], updatedAt: new Date().toISOString() };
      case "delete_tune_chat_session":
        return null;
      case "tune_chat_turn":
        return {
          session: { id: args?.chatId ?? "tune_mock_1", title: "Tune", messages: [{ role: "assistant", content: "Мок-ответ: конфиги выглядят корректно." }], updatedAt: new Date().toISOString() },
          advise: {
            plan: {},
            explanation: "Мок-совет: всё ок.",
            researchLog: [],
            unknownKeys: [],
            diffs: [],
            validationOk: true,
            validationErrors: [],
            validationWarnings: [],
          },
        };

      // --- Graph & Resolve ---
      case "get_graph":
      case "refresh_graph": {
        const p: string = args?.path ?? CHOSEN_MODPACK.path;
        return getGraphForPath(p);
      }
      case "get_resolve_change_plan":
        return { summary: "Исправить дубликат Sodium/Embeddium", risk: "low", actions: [{ RemoveMod: { nodeId: "mod:embeddium_dup" } }], requiresSnapshot: true };
      case "apply_resolve_action":
      case "apply_resolve_change_plan":
      case "resolve_missing_dependencies":
        return ["mod:embeddium_dup removed"];
      case "export_graph_dot":
        return "digraph G { \"mod:create\" -> \"mod:create-aeronautics\"; }";

      // --- Launch ---
      case "launch_profile":
      case "launch_server":
      case "launch_with_quick_play":
        return { exitCode: null, logPath: "/mock/logs/latest.log", instanceId: CHOSEN_MODPACK.path, profile: "client", pid: 12345, startedAt: Date.now() };
      case "list_running_instances":
        return [];
      case "kill_running_instance":
        return "killed";
      case "get_live_debug_stats":
        return { hostCpuPercent: 12.5, hostMemoryUsedMb: 3200, hostMemoryTotalMb: 16384, instance: null };
      case "generate_server_properties":
        return "# Mock server.properties\nonline-mode=true\n";

      // --- Stats & History ---
      case "record_launch":
      case "record_crash":
        return null;
      case "get_launch_stats":
        return { totalLaunches: 12, totalCrashes: 1, totalPlaytimeSeconds: 7320, lastLaunch: new Date().toISOString(), byProfile: [{ id: "client", launches: 12, crashes: 1, playtimeSeconds: 7320, lastLaunch: new Date().toISOString() }] };
      case "get_history_settings":
        return { tracked: {}, focusedScan: false };
      case "update_history_settings":
        return { tracked: args?.settings?.tracked ?? {}, focusedScan: false };
      case "list_project_change_history":
        return {
          entries: [
            {
              id: "evt_1",
              snapshotId: MOCK_SNAPSHOTS[0].id,
              operation: "add_mod",
              reason: "Установка Create: Aeronautics",
              createdAt: new Date(Date.now() - 1000 * 60 * 60 * 2).toISOString(),
              path: "mods/create-aeronautics-0.1.1.jar",
              category: "mods",
              kind: "add",
              preview: "+ create-aeronautics-0.1.1.jar",
              diff: "diff --git a/mods/create-aeronautics-0.1.1.jar b/mods/create-aeronautics-0.1.1.jar\n+ added",
              canOpen: true,
              tags: ["mods"],
              crashFingerprintKey: null,
              planSource: null,
              actor: "user",
              op: "add",
              episodeId: "ep_1",
              fixMethod: null,
              logPath: null,
            },
          ],
          episodes: [
            {
              id: "ep_1",
              outcome: "open",
              fixMethod: "manual",
              fingerprintKey: null,
              startedAt: new Date(Date.now() - 1000 * 60 * 60 * 3).toISOString(),
              endedAt: null,
              summary: "Добавление Aeronautics — без крашей",
              actionIds: ["evt_1"],
              planSource: null,
              snapshotId: MOCK_SNAPSHOTS[0].id,
              resolutionSummary: null,
              logPath: null,
            },
          ],
        };
      case "get_history_entry_diff":
        return "diff --git mock\n+ mocked\n";
      case "read_project_history_file":
        return { path: args?.relativePath ?? "mock", content: "# mock file\n" };
      case "create_tracked_history_snapshot":
        return MOCK_SNAPSHOTS[0];
      case "rollback_history_file":
        return null;
      case "scan_project_changes":
        return { events: [], baselineUpdated: false, added: 0, modified: 0, removed: 0, jarDrift: 0 };
      case "list_recent_pack_events":
        return [];
      case "explain_pack_change":
      case "explain_history_episode":
        return {};

      // --- Snapshots ---
      case "list_snapshots":
        return MOCK_SNAPSHOTS;
      case "create_snapshot":
        return { ...MOCK_SNAPSHOTS[0], id: "snap_new_" + Date.now(), name: args?.name ?? "Snapshot", createdAt: new Date().toISOString() };
      case "diff_snapshots":
        return { addedFiles: ["mods/new.jar"], removedFiles: [], modifiedFiles: ["mods/create-0.5.1.jar"] };
      case "rollback_snapshot":
        return MOCK_SNAPSHOTS[0];
      case "delete_snapshot":
        return null;
      case "get_snapshot_detail":
        return {
          snapshot: MOCK_SNAPSHOTS[0],
          actionsSummary: ["Добавлен Create: Aeronautics"],
          relatedEvents: [],
          planActions: [],
          humanExplanation: "Мок-снапшот до установки Aeronautics",
          changedFiles: [{ path: "mods/create-aeronautics-0.1.1.jar", category: "mods" }],
          manifestOnly: false,
        };
      case "diff_manifest_snapshots":
        return { mcVersionChanged: false, loaderVersionChanged: false, addedMods: ["create-aeronautics"], removedMods: [], diffText: "+ create-aeronautics" };
      case "get_snapshot_file_diff":
        return { path: args?.relativePath ?? "mock", fromExists: true, toExists: true, text: "mock diff" };
      case "diff_snapshot_vs_current":
        return { snapshotChangedFiles: [], snapshotGoneFiles: [], currentAddedFiles: [], manifestCompared: true, manifestDiff: "" };
      case "get_snapshot_file_vs_current_diff":
        return { path: args?.relativePath ?? "mock", fromExists: true, toExists: true, text: "mock diff" };
      case "prune_auto_snapshots":
        return { removedIds: [], totalBytes: 0 };

      // --- Backups ---
      case "create_project_backup":
        return { id: "bak_1", name: args?.name ?? "Backup", createdAt: new Date().toISOString(), sizeBytes: 123456, fileCount: 42 };
      case "list_backups":
        return [{ id: "bak_1", name: "Backup 1", createdAt: new Date().toISOString(), sizeBytes: 123456, fileCount: 42 }];
      case "delete_backup":
      case "restore_backup":
        return null;

      // --- Pack Diff ---
      case "compare_pack_states":
        return {
          report: {
            addedMods: [],
            removedMods: [],
            updatedMods: [],
            changedConfigPaths: [],
            nameA: "A",
            nameB: "B",
            mcA: "1.20.1",
            mcB: "1.20.1",
            loaderA: "fabric 0.15.7",
            loaderB: "fabric 0.15.7",
          },
          configDiffs: [],
        };

      // --- Worlds ---
      case "list_worlds":
        return MOCK_WORLDS;
      case "read_world_info":
        return {
          name: args?.worldName ?? MOCK_WORLDS[0].name,
          seed: 123456789,
          gameType: "survival",
          difficulty: "normal",
          lastPlayed: Date.now() - 1000 * 60 * 30,
          time: 6000,
          spawnX: 0,
          spawnY: 64,
          spawnZ: 0,
          hardcore: false,
          cheatsEnabled: true,
          sizeBytes: 48234,
          sizeFormatted: "48 MB",
        };
      case "backup_world":
        return "/mock/backup.zip";
      case "restore_world_backup":
        return "restored";
      case "delete_world":
        return null;
      case "list_world_backups":
        return [{ file: "backup_2025.zip", worldName: MOCK_WORLDS[0].name, size: 12345, sizeFormatted: "12 KB", createdAt: Math.floor(Date.now() / 1000) }];
      case "delete_world_backup":
        return null;
      case "read_world_icon":
        return null;
      case "list_screenshots":
        return [];
      case "delete_screenshot":
        return null;
      case "open_mca_selector":
        return null;
      case "list_world_dimensions":
        return ["overworld", "nether", "end"];
      case "read_world_map": {
        // tiny fake map 1 region
        const chunks = Array.from({ length: 1024 }, (_, i) => ({
          present: i % 7 === 0 ? 1 : 0,
          lastModified: Date.now() / 1000 - i * 100,
          status: 1,
        }));
        return {
          regions: [{ regionX: 0, regionZ: 0, present: 140, minModified: Date.now() / 1000 - 100000, maxModified: Date.now() / 1000, chunks }],
          minRegionX: 0,
          minRegionZ: 0,
          maxRegionX: 0,
          maxRegionZ: 0,
          totalPresent: 140,
          regionCount: 1,
          dimension: args?.dimension ?? "overworld",
        };
      }
      case "delete_world_chunks":
      case "copy_world_chunks":
      case "paste_world_chunks":
      case "purge_world_regions":
      case "export_world_chunks":
      case "import_world_chunks":
      case "select_world_by_query":
      case "render_world_map_png":
      case "warm_world_map_cache":
      case "clear_world_map_cache":
      case "swap_world_chunks":
      case "change_world_chunks":
      case "read_chunk_editor":
      case "write_chunk_editor":
      case "filter_world_chunks_advanced":
        if (cmd === "copy_world_chunks") return { sourceWorld: args?.worldName ?? "world", chunks: [], bounds: [0, 0, 0, 0] };
        if (cmd === "select_world_by_query") return [];
        if (cmd === "read_chunk_editor") return { regionX: 0, regionZ: 0, index: 0, chunkX: 0, chunkZ: 0, layer: "region", root: { tagType: 10, name: "", children: [] } };
        if (cmd === "filter_world_chunks_advanced") return [];
        return 0;

      // --- Recipes ---
      case "scan_mod_recipes":
        return {
          recipes: [
            {
              id: "create:cutting/stone",
              recipeType: "create:cutting",
              category: "create:cutting",
              modSource: "create",
              sourceFile: "data/create/recipes/cutting/stone.json",
              layout: {
                category: "create:cutting",
                shapeless: true,
                grid: [{ id: "minecraft:cobblestone", count: 1 } as any, null, null, null, null, null, null, null, null],
                output: { id: "minecraft:stone", count: 1 } as any,
                outputCount: 1,
              },
              inputIds: ["minecraft:cobblestone"],
              outputId: "minecraft:stone",
              isConditional: false,
            },
          ],
          jarCount: 9,
          datapackFiles: 124,
          truncated: false,
          totalScanned: 124,
          vanillaJarFound: true,
        };
      case "get_item_icon":
        return null;
      case "get_item_icons_batch":
        return {};
      case "get_recipe_runtime_status":
        return { connected: false, supported: false, message: "Runtime не подключён (мок)" };
      case "get_recipe_runtime_snapshot":
        return { recipes: [], jarCount: 0, datapackFiles: 0, truncated: false, totalScanned: 0, source: "runtime", generatedAt: new Date().toISOString(), protocolVersion: 1, categories: [] } as any;
      case "write_kubejs_recipe_removes":
      case "write_kubejs_craft_recipe":
      case "write_kubejs_tag_edits":
        return "kubejs/server_scripts/mock.js";
      case "list_item_tags":
        return ["c:ingots/iron", "c:ingots/copper", "forge:ingots"];
      case "list_item_catalog":
        return [
          { id: "minecraft:iron_ingot", name: "Iron Ingot", modNs: "minecraft" },
          { id: "create:cogwheel", name: "Cogwheel", modNs: "create" },
        ];
      case "get_item_tag_entries":
        return ["minecraft:iron_ingot", "create:iron_sheet"];
      case "generate_kubejs_recipe_script":
        return { kind: args?.kind ?? "crafting", filename: "server_scripts/mock.js", content: "// mock kubejs" };

      // --- Vanilla jar ---
      case "get_vanilla_client_jar_status":
        return { found: true, version: "1.20.1", resolvedVersion: "1.20.1", jarPath: "/mock/client.jar", downloadSize: 12345 };
      case "download_vanilla_client_jar":
        return "/mock/client.jar";

      // --- Diagnostics ---
      case "scan_ore_generation":
        return [];
      case "audit_performance":
        return [{ code: "PERF_SODIUM", message: "Sodium уже оптимизирует рендер", severity: "Info" }];
      case "detect_duplicate_items":
        return [];
      case "generate_unify_config":
        return {};
      case "find_class_in_mods":
      case "find_dependents_on_class":
        return [];
      case "has_crashed":
        return false;
      case "get_crash_diagnosis":
        return { findings: [], supportMessageDiscord: "", supportMessageGithub: "", modsAdded: [], modsRemoved: [], suspectedMods: [], mcreatorMods: [], classFinderResults: [] };
      case "create_crash_fix_plan":
        return { summary: "Мок-план: удалить дубликат", risk: "low", actions: [], requiresSnapshot: false };
      case "apply_crash_fix_plan":
        return [];
      case "run_crash_assistant_full":
        return { findings: [], supportMessageDiscord: "", supportMessageGithub: "", modsAdded: [], modsRemoved: [], suspectedMods: [], mcreatorMods: [], classFinderResults: [] };
      case "build_ai_crash_context":
      case "analyze_crash_with_ai":
        return {};
      case "get_pack_health":
        return { diagnostics: { errors: 0, warnings: 1 }, exportIssues: [], wrongLoaderCount: 0, duplicateGroups: [{ modId: "sodium", keepCandidate: "sodium-fabric-0.5.8+mc1.20.1.jar", count: 2 }], questIssues: 0, lastCrash: null, overall: "warnings" };
      case "apply_action_plan":
        return {};
      case "record_crash_ai_feedback":
        return "ok";
      case "draft_authored_crash_case":
      case "save_authored_crash_case":
        return {};
      case "list_authored_crash_cases":
        return [];
      case "get_authored_case_export":
        return "mock export";
      case "open_authored_kb_folder":
        return null;
      case "save_problematic_mods_config":
        return null;
      case "get_problematic_mods_config":
        return [];

      // --- Quests ---
      case "load_quest_book":
        return MOCK_QUEST_BOOK;
      case "save_quest_chapter_raw":
      case "save_quest_reward_table":
      case "save_quest_book_data":
      case "save_quest_chapter_groups":
      case "save_quest_locale":
        return { relativePath: "mock.snbt", entryCount: 1, questCount: 1 } as any;
      case "preview_quest_chapter_snbt":
        return args?.jsonPayload ?? "{}";
      case "read_quest_chapter_text":
        return "{}";
      case "validate_quest_book":
        return [];
      case "list_quest_item_catalog":
        return ["minecraft:iron_ingot", "create:cogwheel", "create:water_wheel"];
      case "list_quest_progress_teams":
        return [];
      case "load_quest_progress":
        return { world: "world", teamId: "team", name: "Team", statuses: {}, completedCount: 0, startedCount: 0 };
      case "simulate_quest_progress":
        return { world: "world", teamId: "team", name: "Team", statuses: {}, completedCount: 0, startedCount: 0 };
      case "parse_and_merge_quest_plan":
      case "generate_quest_plan_from_prompt":
      case "generate_quest_line":
      case "filter_and_merge_quest_plan":
        return {
          plan: { schemaVersion: 1, humanExplanation: "Мок-план", confidence: 0.9, chapters: [] },
          validation: { valid: true, errors: [], warnings: [] },
          book: MOCK_QUEST_BOOK,
          touchedChapterIds: [],
          notes: [],
        };
      case "list_quest_chat_sessions":
        return { sessions: [], corruptSkipped: 0 };
      case "new_quest_chat_session":
        return { id: "qc_mock_1", title: args?.title ?? "Новый чат", messages: [], updatedAt: new Date().toISOString() };
      case "load_quest_chat_session":
        return { id: args?.chatId ?? "qc_mock_1", title: "Mock Quest Chat", messages: [], updatedAt: new Date().toISOString() };
      case "save_quest_chat_session":
      case "delete_quest_chat_session":
      case "cancel_quest_chat_turn":
        return null;
      case "quest_chat_turn":
        return {
          session: { id: args?.chatId ?? "qc_mock_1", title: "Quest Chat", messages: [{ role: "assistant", content: "Мок-ответ: квесты выглядят хорошо." }], updatedAt: new Date().toISOString() },
          merge: { plan: { schemaVersion: 1, humanExplanation: "Мок", confidence: 0.9, chapters: [] }, validation: { valid: true, errors: [], warnings: [] }, book: MOCK_QUEST_BOOK, touchedChapterIds: [], notes: [] },
          progressLog: ["мок-лог"],
        };
      case "validate_quest_plan":
        return { valid: true, errors: [], warnings: [] };
      case "quest_plan_system_prompt":
        return "Mock system prompt";
      case "quest_kubejs_list_scripts":
        return [];
      case "quest_kubejs_audit":
        return { linked: 0, missing: 0, orphan: 0, bindings: [], orphanHandlers: [], scripts: [] };
      case "quest_kubejs_read_script":
        return "// mock kubejs";
      case "quest_kubejs_ensure_managed":
        return "kubejs/server_scripts/tuffbox_managed.js";
      case "quest_kubejs_render_template":
        return "// template";
      case "quest_kubejs_append_handler":
        return { relativePath: "kubejs/server_scripts/tuffbox_managed.js", snapshotId: "snap_mock" };

      // --- Export ---
      case "export_modrinth_pack":
      case "export_server_pack":
      case "export_prism_instance":
      case "export_curseforge_pack":
      case "export_packwiz_pack":
        return { path: "/tmp/mock_pack.mrpack", fileCount: 42, overrideCount: 12, warnings: [] };
      case "batch_export_all":
        return [{ kind: "modrinth", status: "ok", path: "/tmp/mock.mrpack", files: 42, overrideCount: 12 }];
      case "export_project_report":
        return {};
      case "validate_modrinth_export":
      case "validate_curseforge_export":
        return [];
      case "reveal_export_path":
        return null;

      // --- GitHub pack transport ---
      case "github_pack_parse_source":
        return { owner: "owner", repo: "repo", ref: null };
      case "github_pack_inspect_source":
        return {
          owner: "tuffbox",
          repo: "example-pack",
          fullName: "tuffbox/example-pack",
          description: "Мок-пакет для превью",
          defaultBranch: "main",
          htmlUrl: "https://github.com/tuffbox/example-pack",
          packVersion: "0.4.2",
          status: "ready",
          ready: true,
          projectName: CHOSEN_MODPACK.name,
          projectVersion: CHOSEN_MODPACK.version,
          mcVersion: CHOSEN_MODPACK.mcVersion,
          loaderKind: CHOSEN_MODPACK.loader,
          loaderVersion: CHOSEN_MODPACK.loaderVersion,
          modCount: 9,
          totalAssetBytes: 12345678,
          customModCount: 1,
        };
      case "github_pack_auth_status":
        return false;
      case "github_pack_start_device_code":
        return { userCode: "MOCK-CODE", verificationUri: "https://github.com/login/device", message: "Мок", expiresIn: 900, interval: 5 };
      case "github_pack_poll_device_code":
        return "mock-token";
      case "github_pack_stage_preview":
        return { packVersion: "0.4.2", manifestFile: ".tuffbox.json", fileCount: 42, managedFiles: [], shareUrl: null, contentDigest: "mock", hasExternalAssets: false };
      case "github_pack_publish":
        return {};
      case "github_pack_install":
        return {};
      case "github_pack_check_update":
        return { updateAvailable: false, installedVersion: "0.4.2", remoteCommitSha: "abc123", reason: "mock" };
      case "github_pack_preview_update":
        return { repo: "tuffbox/example", installedVersion: "0.4.2", incomingVersion: "0.4.3", remoteCommitSha: "abc123", requiresFullReinstall: false, customFiles: false, changes: [] };
      case "github_pack_apply_update":
        return { ok: true, snapshotId: "snap_mock", version: "0.4.3", changes: [] };

      // --- Modpack library ---
      case "get_modrinth_pack_download":
        return "https://cdn.modrinth.com/mock.mrpack";
      case "install_modpack":
        return { path: "/tmp/mock_instance", download: {}, dedup: { mode: "shared", linked: 2, bytesReclaimed: 1234567 } };

      // --- Youtube ---
      case "youtube_my_feed_lookup":
        return { channelId: "UC_mock", label: "Mock Channel" };
      case "youtube_my_feed_fetch":
        return { videos: [], errors: [] };

      // --- Release ---
      case "generate_release_changelog":
        return "# Changelog mock\n- Added Aeronautics\n";
      case "generate_github_release":
        return {};
      case "update_project_version":
        return {
          id: CHOSEN_MODPACK.id,
          name: CHOSEN_MODPACK.name,
          version: args?.version ?? "0.4.3",
          minecraftVersion: CHOSEN_MODPACK.mcVersion,
          loaderKind: CHOSEN_MODPACK.loader,
          loaderVersion: CHOSEN_MODPACK.loaderVersion,
          javaPath: null,
          memoryMb: 4096,
          jvmArgs: [],
          playerName: "Aviator",
          manifestPath: CHOSEN_MODPACK.path,
        };
      case "create_release_snapshot":
        return { snapshot: MOCK_SNAPSHOTS[0], changelogPath: "/mock/CHANGELOG.md" };
      case "list_release_artifacts":
        return [];
      case "create_release_draft":
        return { draftPath: "/mock/draft", metadataPath: "/mock/meta.json", artifactCount: 0 };
      case "generate_lockfile":
        return { schemaVersion: "0.1.0", projectId: CHOSEN_MODPACK.id, projectVersion: CHOSEN_MODPACK.version, minecraftVersion: CHOSEN_MODPACK.mcVersion, loader: { kind: CHOSEN_MODPACK.loader, version: CHOSEN_MODPACK.loaderVersion }, javaMajor: 17, mods: [], graph: { nodeCount: 9, edgeCount: 5, edges: [] }, generatedAt: new Date().toISOString() };

      // --- Import ---
      case "import_project":
      case "import_curseforge_project":
        return "/tmp/mock_import";
      case "search_curseforge_modpacks":
        return [];
      case "get_curseforge_modpack_files":
        return [];

      // --- Content packs ---
      case "list_content_packs":
        return [];
      case "set_content_pack_enabled":
        return { name: "mock", fileName: args?.fileName ?? "mock.zip", enabled: args?.enabled ?? true, kind: args?.folder ?? "resourcepacks", size: 1234, sizeFormatted: "1.2 KB" };

      // --- Servers ---
      case "list_mc_servers":
        return [{ name: "Hypixel", address: "mc.hypixel.net", icon: null, acceptTextures: 1 }];
      case "add_mc_server":
      case "remove_mc_server":
        return [{ name: "Hypixel", address: "mc.hypixel.net", icon: null, acceptTextures: 1 }];
      case "ping_mc_server":
        return { address: args?.address ?? "mc.hypixel.net", online: true, latencyMs: 42, error: null, playersOnline: 12345, playersMax: 20000 };

      // --- Instance ---
      case "create_instance":
        return "/tmp/new_instance/.tuffbox.json";
      case "update_project_settings":
        return null;
      case "get_instance_size":
        return "1.2 GB";

      // --- Logs ---
      case "get_launch_log":
        return "[12:00:00] [main/INFO]: Mock log — Aeronautics loaded\n[12:00:01] [main/INFO]: Done!\n";
      case "list_instance_logs":
        return [{ name: "latest.log", size: 1234, modified: Date.now() }];
      case "read_instance_log":
        return "[12:00:00] Mock log content\n";
      case "create_logs_zip":
        return "/tmp/logs.zip";
      case "capture_test_run_logs":
        return "/tmp/test_logs.zip";

      // --- Test runs ---
      case "list_test_runs":
        return [
          { id: "run_1", profile: "client", startedAt: new Date(Date.now() - 1000 * 60 * 60).toISOString(), status: "passed", logPath: "/mock/log", durationSeconds: 42, verdictReason: "ok", peakProcMb: 2800, peakHostMb: 3200, recommendedRamGb: 4 },
        ];
      case "finalize_test_run":
        return { id: args?.runId ?? "run_1", profile: "client", startedAt: new Date().toISOString(), status: args?.status ?? "passed", logPath: "/mock/log", durationSeconds: 42, verdictReason: null, peakProcMb: 2800, peakHostMb: 3200, recommendedRamGb: 4 };

      // --- Templates ---
      case "save_as_template":
        return null;
      case "list_templates":
        return [{ id: "template_create", name: "Create Base", minecraftVersion: "1.20.1", loader: "fabric", description: "Базовый шаблон Create" }];

      // --- System ---
      case "get_app_version":
        return "0.1.0-mock";
      case "check_for_app_update":
        return { available: false, version: "0.1.0" };
      case "get_home_dir":
        return "/home/user";
      case "get_minecraft_versions":
        return [
          { id: "1.20.1", type: "release" },
          { id: "1.20.4", type: "release" },
          { id: "1.21.1", type: "release" },
        ];
      case "get_loader_versions":
        if ((args?.loader ?? "") === "fabric") return [{ version: "0.15.7", stable: true }, { version: "0.14.25", stable: true }];
        if ((args?.loader ?? "") === "forge") return [{ version: "47.1.3", stable: true }];
        if ((args?.loader ?? "") === "neoforge") return [{ version: "47.1.106", stable: true }];
        return [{ version: "1.0.0", stable: true }];
      case "find_java_runtimes":
        return [{ path: "/usr/lib/jvm/java-17-openjdk/bin/java", version: "17.0.9", major: 17 }];
      case "ensure_java_runtime":
        return { path: "/usr/lib/jvm/java-17-openjdk/bin/java", version: "17.0.9", major: 17 };
      case "get_java_version":
        return "17.0.9";
      case "get_default_java_version":
        return "17.0.9";
      case "get_keyboard_shortcuts":
        return [];
      case "get_download_progress":
        return [];

      // --- Pinning ---
      case "pin_project":
        return null;
      case "is_project_pinned":
        return false;

      // --- File ops ---
      case "open_project_folder":
        return null;
      case "delete_project":
      case "clone_project":
        return "/tmp/cloned/.tuffbox.json";
      case "rename_project":
        return args?.newName ?? "Renamed";
      case "create_project_desktop_shortcut":
        return "/home/user/Desktop/mock.desktop";
      case "take_pending_launch_project":
      case "take_pending_install_repo":
        return null;

      // --- L10n ---
      case "localize":
        return args?.key ?? "";
      case "list_localizations":
        return [];

      // --- Auth ---
      case "mc_start_device_code":
        return { userCode: "MOCK", verificationUri: "https://microsoft.com/link", loginUrl: "https://microsoft.com/link?otc=MOCK", message: "Mock", expiresIn: 900, interval: 5 };
      case "mc_poll_device_code":
        return { profile: { uuid: "mock-uuid-aviator", name: "Aviator", skinUrl: null, capeUrl: null, capes: [] } };
      case "mc_get_microsoft_login_url":
        return "https://login.microsoftonline.com/mock";
      case "mc_login_with_auth_url":
      case "mc_start_microsoft_webview_auth":
      case "mc_offline_login":
        return { profile: { uuid: "mock-uuid-aviator", name: args?.username ?? "Aviator", skinUrl: null, capeUrl: null, capes: [] } };
      case "mc_get_auth_status":
        return {
          loggedIn: true,
          profile: { uuid: "mock-uuid-aviator", name: "Aviator", skinUrl: null, capeUrl: null, capes: [] },
          expiresAt: Date.now() + 1000 * 60 * 60 * 24,
          loginType: "offline",
          skinSource: "mojang",
          capeProvider: "mojang",
          accounts: [{ uuid: "mock-uuid-aviator", name: "Aviator", loginType: "offline", skinSource: "mojang", addedAt: Date.now() }],
          activeAccountUuid: "mock-uuid-aviator",
        };
      case "mc_logout":
        return {
          loggedIn: false,
          profile: null,
          expiresAt: null,
          loginType: "offline",
          skinSource: "mojang",
          capeProvider: "mojang",
          accounts: [],
          activeAccountUuid: null,
        };
      case "mc_refresh_profile":
        return { uuid: "mock-uuid-aviator", name: "Aviator", skinUrl: null, capeUrl: null, capes: [] };
      case "mc_refresh_token":
        return {
          loggedIn: true,
          profile: { uuid: "mock-uuid-aviator", name: "Aviator", skinUrl: null, capeUrl: null, capes: [] },
          expiresAt: Date.now() + 1000 * 60 * 60 * 24,
          loginType: "offline",
          skinSource: "mojang",
          capeProvider: "mojang",
          accounts: [{ uuid: "mock-uuid-aviator", name: "Aviator", loginType: "offline", skinSource: "mojang", addedAt: Date.now() }],
          activeAccountUuid: "mock-uuid-aviator",
        };
      case "mc_get_skin_path":
        return "/mock/skin.png";
      case "mc_fetch_skin_url":
      case "mc_fetch_skin_for_username":
        return null;
      case "mc_set_skin_source":
        return null;
      case "mc_list_accounts":
        return [{ uuid: "mock-uuid-aviator", name: "Aviator", loginType: "offline", skinSource: "mojang", addedAt: Date.now() }];
      case "mc_switch_account":
      case "mc_remove_account":
        return {
          loggedIn: true,
          profile: { uuid: "mock-uuid-aviator", name: "Aviator", skinUrl: null, capeUrl: null, capes: [] },
          expiresAt: Date.now() + 1000 * 60 * 60 * 24,
          loginType: "offline",
          skinSource: "mojang",
          capeProvider: "mojang",
          accounts: [{ uuid: "mock-uuid-aviator", name: "Aviator", loginType: "offline", skinSource: "mojang", addedAt: Date.now() }],
          activeAccountUuid: "mock-uuid-aviator",
        };
      case "mc_apply_skin":
      case "mc_upload_skin":
      case "mc_upload_skin_file":
        return {
          loggedIn: true,
          profile: { uuid: "mock-uuid-aviator", name: "Aviator", skinUrl: "https://mock/skin.png", capeUrl: null, capes: [] },
          expiresAt: Date.now() + 1000 * 60 * 60 * 24,
          loginType: "offline",
          skinSource: "mojang",
          capeProvider: "mojang",
          accounts: [{ uuid: "mock-uuid-aviator", name: "Aviator", loginType: "offline", skinSource: "mojang", addedAt: Date.now() }],
          activeAccountUuid: "mock-uuid-aviator",
        };
      case "mc_apply_cape":
        return {
          loggedIn: true,
          profile: { uuid: "mock-uuid-aviator", name: "Aviator", skinUrl: null, capeUrl: "https://mock/cape.png", capes: [] },
          expiresAt: Date.now() + 1000 * 60 * 60 * 24,
          loginType: "offline",
          skinSource: "mojang",
          capeProvider: "mojang",
          accounts: [{ uuid: "mock-uuid-aviator", name: "Aviator", loginType: "offline", skinSource: "mojang", addedAt: Date.now() }],
          activeAccountUuid: "mock-uuid-aviator",
        };
      case "mc_list_capes":
        return { selectedProvider: "mojang", displayUrl: null, offers: [] };
      case "mc_set_cape_provider":
        return {
          loggedIn: true,
          profile: { uuid: "mock-uuid-aviator", name: "Aviator", skinUrl: null, capeUrl: null, capes: [] },
          expiresAt: Date.now() + 1000 * 60 * 60 * 24,
          loginType: "offline",
          skinSource: "mojang",
          capeProvider: args?.provider ?? "mojang",
          accounts: [{ uuid: "mock-uuid-aviator", name: "Aviator", loginType: "offline", skinSource: "mojang", addedAt: Date.now() }],
          activeAccountUuid: "mock-uuid-aviator",
        };
      case "mc_check_entitlement":
        return true;
      case "mc_get_skin_base64":
        return "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+ip1sAAAAASUVORK5CYII=";
      case "mc_list_yggdrasil_presets":
        return [];
      case "mc_yggdrasil_login":
        return { profile: { uuid: "mock-uuid-aviator", name: args?.username ?? "Aviator", skinUrl: null, capeUrl: null, capes: [] } };

      // --- Presence ---
      case "get_presence_settings":
        return { discordRpcEnabled: false, discordClientId: "" };
      case "save_presence_settings":
        return null;
      case "set_discord_presence":
      case "clear_discord_presence":
        return null;
      case "get_launcher_online":
        return {};
      case "get_launcher_recent_sessions":
        return [];
      case "launcher_presence_start":
      case "launcher_presence_stop":
        return {};

      case "get_launcher_settings":
        return {
          theme: "tuffbox-dark",
          potatoPc: false,
          perfAutoDetected: true,
          concurrentDownloads: 4,
          gameResolution: null,
          preLaunchHook: null,
          postExitHook: null,
          wrapperCommand: null,
          runtimePath: null,
          instancesPath: null,
          defaultJavaPath: null,
          javaCustomArgs: null,
          defaultMemoryMb: 4096,
          youtubeInlinePlayer: true,
          showYoutubeOnHome: false,
          newsShowUpdates: true,
          ingameOverlay: false,
          cpuAffinityMode: "off",
          cpuAffinityMask: null,
          gpuPreference: "auto",
          autoHideWorkflowRail: false,
          hideIdeNextBar: false,
          sidebarMode: "full",
          uiScalePercent: 100,
          uiScaleMode: "auto",
          roundedCorners: true,
          homeBackdrop: true,
        };
      case "save_launcher_settings_cmd":
        return args?.settings ?? {};
      case "detect_gpus":
        return [{ id: "gpu0", name: "Mock GPU", vendor: "Mock", kind: "discrete", vramMb: 8192, primary: true, pciSlot: null }];
      case "get_runtime_path_info":
        return { current: "/home/user/TuffBox/runtime", default: "/home/user/TuffBox/runtime" };
      case "get_instances_path_info":
        return { current: "/home/user/TuffBox/instances", default: "/home/user/TuffBox/instances" };
      case "validate_runtime_path_cmd":
      case "validate_instances_path_cmd":
        return true;
      case "get_swarm_settings":
        return { enabled: false, p2pEnabled: false, onboardingDone: true };
      case "get_local_kudos_balance":
        return { totalKudos: 42, rac: 1.2 };
      case "complete_swarm_onboarding":
        return null;
      case "publish_experience_capsule":
        return { published: true, supabaseOk: true, p2pGossipOk: true, hubConfigured: true };
      case "dismiss_share_prompt":
        return null;
      case "cosmetics_get_local_profile":
        return { playerKey: args?.playerKey ?? "mock", username: "Aviator", skinModel: "classic", sharePublic: false, writeSecret: "secret" };
      case "cosmetics_wings_catalog":
        return [{ id: "none", label: "None" }, { id: "dragon", label: "Dragon" }];
      case "cosmetics_hat_catalog":
        return [{ id: "none", label: "None" }, { id: "crown", label: "Crown" }];

      // --- Plugin mocks ---
      default:
        if (cmd.startsWith("plugin:")) {
          // tracing/log is extremely high-frequency — must not spam console
          if (cmd.includes("tracing")) return null;
          if (cmd.includes("dialog") && cmd.includes("open")) return null;
          if (cmd.includes("shell") && cmd.includes("open")) return null;
          if (cmd.includes("dialog") && cmd.includes("confirm")) return true;
          if (cmd.includes("dialog") && cmd.includes("message")) return null;
          // throttle unhandled plugin spam: only log once per cmd
          try {
            const k = `__mock_warned_${cmd}`;
            if (!(globalThis as any)[k]) {
              (globalThis as any)[k] = true;
              console.debug(`[mockIPC] unhandled plugin cmd ${cmd}`, args);
            }
          } catch {}
          return null;
        }
        // silent stubs for shell-polling cmds — prevent warn flood
        if (cmd === "list_background_tasks" || cmd === "set_window_glass" || cmd === "get_crash_fix_banner" || cmd === "get_launch_log") {
          return cmd.startsWith("list_") ? [] : null;
        }
        // throttle unhandled warnings — log once per cmd
        try {
          const rk = `__mock_warned_cmd_${cmd}`;
          if (!(globalThis as any)[rk]) {
            (globalThis as any)[rk] = true;
            console.warn(`[mockIPC] unhandled cmd ${cmd}`, args);
          }
        } catch {
          console.warn(`[mockIPC] unhandled cmd ${cmd}`, args);
        }
        // Heuristic fallback so the UI doesn't crash on missing mocks
        if (cmd.startsWith("list_") || cmd.startsWith("search_") || cmd.startsWith("get_") && cmd.includes("list")) return [];
        if (cmd.startsWith("get_") || cmd.startsWith("list")) return null;
        return null;
    }
  }, { shouldMockEvents: true });

  // Mock some direct window listeners for file drop / etc.
  console.info(`[browserMock] installed — chosen modpack: ${CHOSEN_MODPACK.name} (${CHOSEN_MODPACK.mcVersion} · ${CHOSEN_MODPACK.loader} ${CHOSEN_MODPACK.loaderVersion})`);

  // Visual preview badge — so tester sees which pack is mocked without
  // opening devtools. Injected once, pointer-safe, hidden on potato-pc.
  try {
    const injectBadge = () => {
      if (document.getElementById("browser-mock-badge")) return;
      const el = document.createElement("div");
      el.id = "browser-mock-badge";
      el.textContent = `PREVIEW · ${CHOSEN_MODPACK.name} · ${CHOSEN_MODPACK.mcVersion} · ${CHOSEN_MODPACK.loader} ${CHOSEN_MODPACK.loaderVersion} · 9 модов`;
      el.setAttribute("role", "status");
      el.setAttribute("aria-label", "Browser preview test data");
      el.style.cssText = [
        "position:fixed",
        "bottom:56px",
        "right:12px",
        "z-index:9999",
        "padding:6px 12px",
        "font:600 12px/1.4 var(--font-mono, monospace)",
        "letter-spacing:0.02em",
        "color:var(--text-primary)",
        "background:color-mix(in srgb, var(--accent-primary) 16%, var(--bg-primary))",
        "border:1px solid color-mix(in srgb, var(--accent-primary) 40%, transparent)",
        "border-radius:999px",
        "backdrop-filter:blur(8px)",
        "pointer-events:none",
        "box-shadow:0 4px 12px rgba(0,0,0,0.12)",
      ].join(";");
      (document.body ?? document.documentElement).appendChild(el);
    };
    if (document.readyState === "loading") {
      document.addEventListener("DOMContentLoaded", injectBadge, { once: true });
      // Also try after a short delay — HMR may have already passed DOMContentLoaded
      setTimeout(injectBadge, 400);
    } else {
      // Defer one tick so App.svelte has mounted
      setTimeout(injectBadge, 200);
    }
  } catch {}

  return true;
}

export function isBrowserMockActive(): boolean {
  return installed;
}
