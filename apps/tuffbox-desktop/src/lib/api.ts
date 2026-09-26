import { invoke, isTauri } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { projectPath, type AuthState, type McProfile, type DeviceCodeInfo, type SkinSource, type AccountEntry, type McCapeEntry, type CapeProvider, type CapeCatalog, type YggdrasilPreset, type PresenceSettings, type LauncherSettings } from "./store";
import { get } from "svelte/store";

// ─── Types ──────────────────────────────────────────────────────────

export interface ProjectSummary {
  id: string;
  name: string;
  version: string;
  minecraftVersion: string;
  loaderKind: string;
  loaderVersion: string;
  javaPath: string | null;
  memoryMb: number;
  jvmArgs: string[];
  playerName: string;
  /** Canonical `.tuffbox.json` path (may differ from the path passed to validate). */
  manifestPath: string;
}

export interface SchemaStatus {
  current: string;
  detected: string;
  needsMigration: boolean;
  supported: string[];
}

export interface ConfigFileSummary {
  path: string;
  name: string;
  extension: string;
  size: number;
  modified: number | null;
}

export interface QuestTask {
  id: string;
  type: string;
  title?: string | null;
  value?: unknown;
  properties?: Record<string, unknown>;
  /** False when title came only from lang overlay — omit on chapter SNBT export. */
  titleFromSnbt?: boolean;
}

export interface QuestReward {
  id: string;
  type: string;
  title?: string | null;
  properties?: Record<string, unknown>;
}

export interface QuestData {
  id: string;
  title: string;
  subtitle?: string | null;
  description: string[];
  x: number;
  y: number;
  /** String id or full item-stack compound `{ id, Count, tag, ... }`. */
  icon?: string | Record<string, unknown> | null;
  dependencies: string[];
  tasks: QuestTask[];
  rewards: QuestReward[];
  optional: boolean;
  shape?: string | null;
  size?: number | null;
  hideDependencyLines?: boolean | null;
  hideDependentLines?: boolean | null;
  minRequiredDependencies?: number | null;
  canRepeat?: boolean | null;
  invisible?: boolean | null;
  disableToast?: boolean | null;
  dependencyRequirement?: string | null;
  extras?: Record<string, unknown>;
  /** False when title came only from lang overlay — omit on chapter SNBT export. */
  titleFromSnbt?: boolean;
  /** False when subtitle came only from lang overlay — omit on chapter SNBT export. */
  subtitleFromSnbt?: boolean;
  /** False when description came only from lang overlay — omit on chapter SNBT export. */
  descriptionFromSnbt?: boolean;
}

export interface QuestChapter {
  id: string;
  title: string;
  /** String id or full item-stack compound. */
  icon?: string | Record<string, unknown> | null;
  quests: QuestData[];
  group?: string | null;
  orderIndex?: number | null;
  filename?: string | null;
  defaultQuestShape?: string | null;
  defaultHideDependencyLines?: boolean | null;
  extras?: Record<string, unknown>;
  sourceFile?: string | null;
  /** False when title came only from lang overlay — omit on chapter SNBT export. */
  titleFromSnbt?: boolean;
}

export interface QuestChapterGroup {
  id: string;
  title: string;
  titleFromSnbt?: boolean;
}

/**
 * Prefer omitting via `chapterToSnbtJson`. Safety net: drop locale-sourced text
 * when the corresponding *FromSnbt flag is false.
 */
export function stripLocaleOverlay<T = unknown>(value: T): T {
  if (value === null || typeof value !== "object") return value;
  if (Array.isArray(value)) {
    return value.map((v) => stripLocaleOverlay(v)) as T;
  }
  const obj = value as Record<string, unknown>;
  const out: Record<string, unknown> = { ...obj };
  if (out.titleFromSnbt === false) delete out.title;
  if (out.subtitleFromSnbt === false) delete out.subtitle;
  if (out.descriptionFromSnbt === false) delete out.description;
  delete out.titleFromSnbt;
  delete out.subtitleFromSnbt;
  delete out.descriptionFromSnbt;
  if (Array.isArray(out.quests)) {
    out.quests = out.quests.map((q) => stripLocaleOverlay(q));
  }
  if (Array.isArray(out.tasks)) {
    out.tasks = out.tasks.map((t) => stripLocaleOverlay(t));
  }
  return out as T;
}

function omitEmpty<T extends Record<string, unknown>>(obj: T): T {
  const out: Record<string, unknown> = {};
  for (const [k, v] of Object.entries(obj)) {
    if (v === undefined || v === null) continue;
    if (typeof v === "string" && v.length === 0) continue;
    if (Array.isArray(v) && v.length === 0) continue;
    out[k] = v;
  }
  return out as T;
}

function taskToSnbtJson(t: QuestTask): Record<string, unknown> {
  return omitEmpty({
    id: t.id,
    type: t.type,
    title: t.titleFromSnbt !== false && t.title ? t.title : undefined,
    value: t.value ?? undefined,
    ...(t.properties ?? {}),
  });
}

function rewardToSnbtJson(r: QuestReward): Record<string, unknown> {
  return omitEmpty({
    id: r.id,
    type: r.type,
    title: r.title ?? undefined,
    ...(r.properties ?? {}),
  });
}

function questToSnbtJson(q: QuestData): Record<string, unknown> {
  return omitEmpty({
    id: q.id,
    title: q.titleFromSnbt !== false ? q.title : undefined,
    subtitle: q.subtitleFromSnbt !== false ? (q.subtitle ?? undefined) : undefined,
    description:
      q.descriptionFromSnbt !== false && q.description?.length
        ? q.description
        : undefined,
    x: q.x,
    y: q.y,
    icon: q.icon ?? undefined,
    dependencies: q.dependencies?.length ? q.dependencies : undefined,
    tasks: q.tasks?.length ? q.tasks.map(taskToSnbtJson) : undefined,
    rewards: q.rewards?.length ? q.rewards.map(rewardToSnbtJson) : undefined,
    optional: q.optional || undefined,
    shape: q.shape ?? undefined,
    size: q.size ?? undefined,
    hide_dependency_lines: q.hideDependencyLines ?? undefined,
    hide_dependent_lines: q.hideDependentLines ?? undefined,
    min_required_dependencies: q.minRequiredDependencies ?? undefined,
    can_repeat: q.canRepeat ?? undefined,
    invisible: q.invisible ?? undefined,
    disable_toast: q.disableToast ?? undefined,
    dependency_requirement: q.dependencyRequirement ?? undefined,
    ...(q.extras ?? {}),
  });
}

/** Convert editor chapter model to FTB Quests SNBT-shaped JSON (before locale strip). */
export function chapterToSnbtJson(ch: QuestChapter): Record<string, unknown> {
  return omitEmpty({
    id: ch.id,
    title: ch.titleFromSnbt !== false ? ch.title : undefined,
    icon: ch.icon ?? undefined,
    group: ch.group ?? undefined,
    order_index: ch.orderIndex ?? undefined,
    filename: ch.filename ?? undefined,
    default_quest_shape: ch.defaultQuestShape ?? undefined,
    default_hide_dependency_lines: ch.defaultHideDependencyLines ?? undefined,
    quests: ch.quests.map(questToSnbtJson),
    ...(ch.extras ?? {}),
  });
}

function joinProjectPath(projectDir: string, relative: string): string {
  const sep = projectDir.includes("\\") ? "\\" : "/";
  const base = projectDir.replace(/[/\\]+$/, "");
  const rel = relative.replace(/^[/\\]+/, "").replace(/[/\\]/g, sep);
  return `${base}${sep}${rel}`;
}

export interface QuestBook {
  chapters: QuestChapter[];
  title?: string | null;
  subtitle?: string | null;
  chapterGroups?: QuestChapterGroup[];
  rewardTables?: QuestRewardTable[];
  bookSettings?: Record<string, unknown>;
  /** locale code → translation keys from `lang/*.snbt`. */
  locales?: Record<string, Record<string, string | string[] | unknown>>;
  activeLocale?: string | null;
  /** Non-fatal problems while loading SNBT (corrupt chapter, bad lang, etc.). */
  loadWarnings?: string[];
}

export interface QuestKubeJsHandler {
  kind: string;
  id: string;
  relativePath: string;
  line: number;
}

export interface QuestKubeJsScriptFile {
  relativePath: string;
  name: string;
  managed: boolean;
}

export interface QuestKubeJsBinding {
  kind: string;
  id: string;
  questId: string;
  questTitle: string;
  chapterId: string;
  title?: string | null;
  status: "linked" | "missing" | string;
  handlers: QuestKubeJsHandler[];
}

export interface QuestKubeJsAudit {
  linked: number;
  missing: number;
  orphan: number;
  bindings: QuestKubeJsBinding[];
  orphanHandlers: QuestKubeJsHandler[];
  scripts: QuestKubeJsScriptFile[];
}

export interface QuestKubeJsTemplateParams {
  kind: string;
  id: string;
  maxProgress?: number | null;
  blockId?: string | null;
  itemId?: string | null;
  count?: number | null;
  title?: string | null;
}

export interface QuestRewardTable {
  id: string;
  title?: string | null;
  /** Full FTB reward compounds (type/item/NBT preserved). */
  rewards: Record<string, unknown>[];
  emptyWeight?: number;
  sourceFile?: string | null;
  extras?: Record<string, unknown>;
}

/** Display id for UI; works with string or stack compound. */
export function iconDisplayId(
  icon: string | Record<string, unknown> | null | undefined,
): string | null {
  if (!icon) return null;
  if (typeof icon === "string") return icon.trim() || null;
  const id = icon.id ?? icon.item;
  return typeof id === "string" && id.trim() ? id.trim() : null;
}

/** Read weight from a reward-table entry compound. */
export function rewardEntryWeight(entry: Record<string, unknown>): number {
  const w = entry.weight;
  if (typeof w === "number") return w;
  if (typeof w === "string") {
    const n = Number(w);
    return Number.isFinite(n) ? n : 1;
  }
  return 1;
}

/** Read id from a reward-table entry compound. */
export function rewardEntryId(entry: Record<string, unknown>): string {
  const id = entry.id;
  return typeof id === "string" ? id : "";
}

export interface QuestValidationIssue {
  questId: string;
  message: string;
}

/** AI → launcher contract for FTB Quests (see quest_plan.rs). */
export interface QuestPlan {
  schemaVersion: number;
  humanExplanation: string;
  confidence: number;
  needsUserReview?: boolean;
  source?: string | null;
  chapterGroups?: QuestChapterGroup[];
  rewardTables?: QuestPlanRewardTable[];
  chapters: QuestPlanChapter[];
}

export interface QuestPlanRewardTable {
  id: string;
  title?: string | null;
  entries: { rewardId: string; weight: number }[];
  emptyWeight?: number;
}

export interface AiTokenUsage {
  promptTokens?: number | null;
  completionTokens?: number | null;
  totalTokens?: number | null;
}

export interface QuestChatMessage {
  role: string;
  content: string;
  createdAt?: string | null;
  plan?: QuestPlan | null;
  progressLog?: string[] | null;
  usage?: AiTokenUsage | null;
}

export interface QuestChatSession {
  id: string;
  title: string;
  messages: QuestChatMessage[];
  pendingPlan?: QuestPlan | null;
  updatedAt: string;
}

export interface TuneChatMessage {
  role: string;
  content: string;
  createdAt?: string | null;
}

export interface TunePendingAdvise {
  plan: Record<string, unknown>;
  explanation: string;
  researchLog?: Record<string, unknown>[];
  unknownKeys?: Record<string, unknown>[];
  diffs?: Record<string, unknown>[];
  validationOk?: boolean;
  validationErrors?: string[];
  validationWarnings?: string[];
}

export interface TuneChatSession {
  id: string;
  title: string;
  messages: TuneChatMessage[];
  pendingAdvise?: TunePendingAdvise | null;
  updatedAt: string;
  focusPath?: string | null;
}

export interface TuneChatTurnResult {
  session: TuneChatSession;
  advise: {
    plan: Record<string, unknown>;
    explanation: string;
    researchLog: { step: string; detail: string; ok: boolean; url?: string | null }[];
    unknownKeys: { path: string; key: string; modHint?: string | null }[];
    diffs: {
      path: string;
      patchType: string;
      beforeExcerpt: string;
      afterExcerpt: string;
      ok: boolean;
      error?: string | null;
    }[];
    validationOk: boolean;
    validationErrors: string[];
    validationWarnings: string[];
  };
}

export interface QuestChatTurnResult {
  session: QuestChatSession;
  merge: QuestPlanMergeResult;
  progressLog: string[];
  usage?: AiTokenUsage | null;
}

export interface QuestPlanChapter {
  id?: string | null;
  title: string;
  icon?: string | null;
  group?: string | null;
  orderIndex?: number | null;
  mode?: "upsert" | "replace" | string | null;
  quests: QuestPlanQuest[];
}

export interface QuestPlanQuest {
  id?: string | null;
  title: string;
  subtitle?: string | null;
  description?: string[];
  x?: number;
  y?: number;
  icon?: string | null;
  dependencies?: string[];
  tasks: QuestTask[];
  rewards?: QuestReward[];
  optional?: boolean;
  shape?: string | null;
  size?: number | null;
}

export interface QuestPlanValidation {
  valid: boolean;
  errors: string[];
  warnings: string[];
  bookErrors?: { questId: string; message: string }[];
}

export interface QuestPlanMergeResult {
  plan: QuestPlan;
  validation: QuestPlanValidation;
  book: QuestBook;
  touchedChapterIds: string[];
  notes: string[];
}

export type QuestProgressStatus =
  | "completed"
  | "started"
  | "available"
  | "locked"
  | "unknown";

export interface QuestProgressTeamRef {
  world: string;
  teamId: string;
  name: string;
  relativePath: string;
}

export interface QuestProgressSnapshot {
  world: string;
  teamId: string;
  name: string;
  statuses: Record<string, QuestProgressStatus>;
  completedCount: number;
  startedCount: number;
}

export interface IngredientDisplay {
  id: string;
  kind?: string;
  name?: string;
  count?: number;
  tooltip?: string[];
  iconUrl?: string | null;
  alts?: IngredientDisplay[];
}

export interface RuntimeRecipeSlot {
  role: string;
  name?: string | null;
  x: number;
  y: number;
  width: number;
  height: number;
  ingredients: IngredientDisplay[];
}

export interface RecipeLayout {
  category: string;
  shapeless: boolean;
  grid: (IngredientDisplay | null)[];
  output: IngredientDisplay;
  outputCount: number;
  cookTime?: number;
  experience?: number;
  slots?: RuntimeRecipeSlot[];
}

export interface ScannedRecipe {
  id: string;
  recipeType: string;
  category: string;
  modSource: string;
  sourceFile: string;
  layout: RecipeLayout;
  inputIds: string[];
  outputId: string;
  isConditional: boolean;
}

export interface RecipeScanResult {
  recipes: ScannedRecipe[];
  jarCount: number;
  datapackFiles: number;
  truncated: boolean;
  totalScanned: number;
  /** False when no installed vanilla client jar was found (vanilla recipes missing). */
  vanillaJarFound?: boolean;
}

export interface VanillaClientJarStatus {
  found: boolean;
  version: string;
  resolvedVersion: string;
  jarPath?: string | null;
  downloadSize?: number | null;
}

export interface RecipeRuntimeStatus {
  connected: boolean;
  supported: boolean;
  message: string;
  minecraftVersion?: string | null;
  pid?: number | null;
}

export interface RuntimeRecipeCategory {
  id: string;
  title: string;
  width: number;
  height: number;
  stations: IngredientDisplay[];
}

export interface RecipeRuntimeSnapshot extends RecipeScanResult {
  source: "runtime";
  generatedAt: string;
  protocolVersion: number;
  categories: RuntimeRecipeCategory[];
}

export interface KubeJsScript {
  kind: string;
  filename: string;
  content: string;
}

export interface CraftDraft {
  /** shaped | shapeless | smelting | blasting | smoking | campfire | smithing | stonecutting */
  kind?: string | null;
  shaped: boolean;
  /** Row-major 3×3; null = empty. Tags use `#ns:path`. */
  grid: (string | null)[];
  output: string;
  outputCount: number;
  replaceId?: string | null;
  input?: string | null;
  xp?: number | null;
  cookTime?: number | null;
  template?: string | null;
  base?: string | null;
  addition?: string | null;
}

export interface TagDraft {
  tagId: string;
  add: string[];
  remove: string[];
  removeAll?: boolean;
}

export interface ProfileSummary {
  id: string;
  name: string;
  side: string;
  memoryMb: number | null;
  jvmArgs: string[];
}

export interface PackBrief {
  goal: string;
  targetAudience: string;
  gameplayPillars: string[];
  constraints: string[];
  releaseTargets: string[];
  notes: string;
}

export interface CreateModeBrief {
  title: string;
  mcVersion: string;
  loader: string;
  targetCount: number;
  mustHave: Array<{ name: string; reason: string; facet?: string | null }>;
  categories: Array<{ name: string; budget: number; facet?: string | null }>;
  exclude: string[];
}

export interface ListingGalleryItem {
  path?: string | null;
  url?: string | null;
  caption?: string | null;
}

export interface ProjectListing {
  name: string;
  summary: string;
  bodyMarkdown: string;
  iconPath?: string | null;
  gallery: ListingGalleryItem[];
  categories: string[];
  authors: string[];
}

export interface ModDependencySpec {
  type: "requires" | "optional" | "recommended" | "incompatible" | "embedded";
  target: string;
  versionConstraint?: string | null;
  reason?: string | null;
}

export interface ModInstallPreview {
  projectId: string;
  slug: string;
  name: string;
  version: string;
  fileName: string | null;
  side: string;
  dependencies: ModDependencySpec[];
  /** Dependency targets already present in the project (slug or provider id). */
  installedDependencies?: string[];
  dependents?: { id: string; slug: string; name: string }[];
}

export interface GraphNode {
  id: string;
  kind: string;
  label: string;
  version: string | null;
  side: string;
  metadata: Record<string, string>;
}

export interface GraphEdge {
  from: string;
  to: string;
  kind: string;
  constraint: string | null;
  reason: string | null;
}

export interface DependencyGraph {
  nodes: GraphNode[];
  edges: GraphEdge[];
  source?: "local" | "cache" | "network" | string;
  generatedAt?: string | null;
}

export interface Diagnostic {
  severity: "Info" | "Warning" | "Error" | string;
  code: string;
  message: string;
  relatedNodes: string[];
}

export interface DiagnosticCounts {
  errorCount: number;
  warningCount: number;
  cached: boolean;
}

export interface ChangePlan {
  summary: string;
  risk: string;
  actions: ChangeAction[];
  requiresSnapshot: boolean;
}

export interface ChangeAction {
  InstallMod?: { projectId: string; version?: string | null };
  RemoveMod?: { nodeId: string };
  DisableMod?: { nodeId: string };
  UpdateMod?: { nodeId: string; targetVersion: string };
  EditConfig?: { path: string; patch: string };
}

export interface HistorySettings {
  tracked: Record<string, boolean>;
  focusedScan?: boolean;
}

export interface ProjectChangeEntry {
  id: string;
  snapshotId: string;
  operation: string;
  reason: string;
  createdAt: string;
  path: string;
  category: string;
  kind: string;
  preview: string;
  diff: string;
  canOpen: boolean;
  tags?: string[];
  crashFingerprintKey?: string | null;
  planSource?: string | null;
  actor?: string;
  op?: string;
  episodeId?: string | null;
  fixMethod?: string | null;
  logPath?: string | null;
}

/** Crash → actions → outcome grouping for Smart History. */
export interface HistoryEpisode {
  id: string;
  outcome: "open" | "fixed" | "broke" | "rolled_back" | string;
  fixMethod: "ai" | "heuristic" | "kb" | "swarm" | "manual" | "unknown" | string;
  fingerprintKey?: string | null;
  startedAt: string;
  endedAt?: string | null;
  summary: string;
  actionIds: string[];
  planSource?: string | null;
  snapshotId?: string | null;
  resolutionSummary?: string | null;
  logPath?: string | null;
}

export interface HistoryListResult {
  entries: ProjectChangeEntry[];
  episodes: HistoryEpisode[];
}

export interface PackEvent {
  id: string;
  ts: string;
  actor: string;
  op: string;
  paths: string[];
  category: string;
  summary: string;
  snapshotId?: string | null;
  tags?: string[];
  meta?: Record<string, unknown> | null;
}

export interface ScanProjectChangesResult {
  events: PackEvent[];
  baselineUpdated: boolean;
  added: number;
  modified: number;
  removed: number;
  jarDrift: number;
}

export interface HistoryFileContent {
  path: string;
  content: string;
}

export interface Snapshot {
  id: string;
  name: string;
  createdAt: string;
  reason: string;
  manifestPath: string;
  lockfilePath: string | null;
  changedFiles: string[];
  /** On-disk size of the snapshot directory in bytes (0 when unknown). */
  sizeBytes: number;
  tags?: string[];
  crashFingerprintKey?: string | null;
  reportId?: string | null;
  planSource?: string | null;
  matchedCaseIds?: string[];
  operation?: string;
  actionsSummary?: string[];
  actor?: string | null;
}

export interface SnapshotDiff {
  addedFiles: string[];
  removedFiles: string[];
  modifiedFiles: string[];
}

export interface SnapshotFileDiff {
  path: string;
  fromExists: boolean;
  toExists: boolean;
  text: string;
}

export interface SnapshotVsCurrent {
  snapshotChangedFiles: string[];
  snapshotGoneFiles: string[];
  currentAddedFiles: string[];
  manifestCompared: boolean;
  manifestDiff: string;
}

export interface PruneResult {
  removedIds: string[];
  totalBytes: number;
}

export interface SnapshotChangedFile {
  path: string;
  category: string;
}

export interface SnapshotPlanAction {
  op: string;
  modId?: string | null;
  provider?: string | null;
  projectId?: string | null;
  version?: string | null;
  path?: string | null;
  patchType?: string | null;
  patch?: unknown;
  reason?: string | null;
  risk?: string;
}

export interface SnapshotDetail {
  snapshot: Snapshot;
  actionsSummary: string[];
  relatedEvents: PackEvent[];
  planActions: SnapshotPlanAction[];
  humanExplanation?: string | null;
  changedFiles: SnapshotChangedFile[];
  manifestOnly: boolean;
}

export interface ManifestSnapshotDiff {
  mcVersionChanged?: boolean;
  fromMcVersion?: string;
  toMcVersion?: string;
  loaderVersionChanged?: boolean;
  fromLoaderVersion?: string;
  toLoaderVersion?: string;
  addedMods?: string[];
  removedMods?: string[];
  diffText?: string;
}

// ── Pack Diff (compare builds / snapshots / backups) ────────────────

export type PackSource =
  | { type: "manifest"; path: string }
  | { type: "snapshot"; projectDir: string; snapshotId: string }
  | { type: "backup"; projectDir: string; backupId: string };

export interface PackModRow {
  id: string;
  name: string;
  version: string;
  fileName?: string | null;
}

export interface ModUpdate {
  id: string;
  name: string;
  from: PackModRow;
  to: PackModRow;
}

export interface PackDiffReport {
  addedMods: PackModRow[];
  removedMods: PackModRow[];
  updatedMods: ModUpdate[];
  changedConfigPaths: string[];
  nameA: string;
  nameB: string;
  mcA: string;
  mcB: string;
  loaderA: string;
  loaderB: string;
}

export interface PackStateDiff {
  report: PackDiffReport;
  configDiffs: Array<{ path: string; diffText: string }>;
}

export interface TestRunRecord {
  id: string;
  profile: string;
  startedAt: string;
  status: string;
  logPath: string;
  durationSeconds: number | null;
  verdictReason?: string | null;
  capturedPaths?: string[];
  peakProcMb?: number | null;
  peakHostMb?: number | null;
  recommendedRamGb?: number | null;
}

/// Stable lifecycle phases emitted by the desktop backend on `launch-phase`.
/// A command returning only means spawn preparation completed; controls must
/// use this stream (and `runningInstances`) for their visible state.
export type LaunchPhase =
  | "preflight"
  | "resolving_java"
  | "downloading"
  | "starting"
  | "running"
  | "stopping"
  | "exited"
  | "failed";

/// Structured launch error returned by the launch Tauri commands
/// (mirrors `tuffbox_core::launch_error::LaunchErrorInfo`).
export interface LaunchErrorInfo {
  kind: string;
  message: string;
  logPath?: string;
}

export interface LaunchLifecycleEvent {
  /** Manifest path — the same key used by `list_running_instances`. */
  id: string;
  profile: string;
  phase: LaunchPhase;
  message: string;
  logPath?: string;
  pid?: number;
  startedAt?: number;
  exitCode?: number | null;
  stopped?: boolean;
  error?: LaunchErrorInfo;
}

export interface LaunchCrashEvent {
  id: string;
  /** Compatibility alias for older event consumers. */
  path?: string;
  profile: string;
  /** Flattened compatibility copy of `error`. */
  kind?: string;
  message?: string;
  logPath?: string;
  error: LaunchErrorInfo;
  exitCode?: number | null;
}

export interface ProcessExitedEvent {
  id: string;
  profile?: string;
  startedAt?: number;
  code?: number | null;
  stopped?: boolean;
}

export interface LaunchResult {
  exitCode: number | null;
  logPath: string;
  instanceId: string;
  profile: string;
  pid: number;
  startedAt: number;
}

export interface ExportResult {
  path: string;
  fileCount: number;
  overrideCount: number;
  /** Non-fatal problems: files skipped because they were unreadable. */
  warnings?: string[] | null;
}

export interface ExportIssue {
  severity: "error" | "warning";
  code: string;
  message: string;
  target: string | null;
}

export interface ReleaseSnapshotResult {
  snapshot: Snapshot;
  changelogPath: string;
}

export interface ReleaseArtifactRecord {
  id: string;
  kind: string;
  path: string;
  createdAt: string;
  fileCount: number;
  overrideCount: number;
}

export interface ReleaseDraftResult {
  draftPath: string;
  metadataPath: string;
  artifactCount: number;
}

export interface ModInfo {
  id: string;
  name: string;
  version: string;
  side: string;
  source: string;
  projectId?: string | null;
  fileName?: string | null;
  iconUrl?: string | null;
  clientSide?: string | null;
  serverSide?: string | null;
  contentType?: string;
  /** Present since list_mods started reporting manifest status. */
  disabled?: boolean;
  status?: string[];
}

export interface SearchResult {
  id: string;
  slug: string;
  name: string;
  description: string;
  projectType: string;
  iconUrl?: string | null;
  clientSide?: string | null;
  serverSide?: string | null;
  author?: string | null;
  downloads?: number | null;
  follows?: number | null;
  dateModified?: string | null;
  categories?: string[];
}

export interface CrashAnalysisFinding {
  severity: string;
  code: string;
  title: string;
  description: string;
  autoFix: string | null;
  references: string[];
}

export interface CrashAnalysisReport {
  findings: CrashAnalysisFinding[];
  supportMessageDiscord: string;
  supportMessageGithub: string;
  modsAdded: string[];
  modsRemoved: string[];
  suspectedMods: string[];
  mcreatorMods: string[];
  classFinderResults: ClassMatch[];
}

export interface ClassMatch {
  className: string;
  modId: string;
  modName: string;
}

export interface ModSyncFailure {
  modId: string;
  error: string;
}

export interface ModSyncReport {
  downloaded: string[];
  alreadyPresent: string[];
  skipped: string[];
  failed: ModSyncFailure[];
}

export interface TuffboxLockfile {
  schemaVersion: string;
  projectId: string;
  projectVersion: string;
  minecraftVersion: string;
  loader: { kind: string; version: string };
  javaMajor: number | null;
  mods: LockedMod[];
  graph: LockedGraphData;
  generatedAt: string;
}

export interface LockedMod {
  id: string;
  name: string;
  version: string;
  source: LockedSource;
  fileName: string | null;
  hashes: LockedHashes;
  side: string;
}

export interface LockedSource {
  kind: string;
  projectId: string | null;
  fileId: string | null;
  url: string | null;
  path: string | null;
}

export interface LockedHashes {
  sha1: string | null;
  sha512: string | null;
}

export interface LockedGraphData {
  nodeCount: number;
  edgeCount: number;
  edges: LockedEdgeData[];
}

export interface LockedEdgeData {
  from: string;
  to: string;
  kind: string;
  constraint: string | null;
  reason: string | null;
}

export interface TemplateInfo {
  id: string;
  name: string;
  minecraftVersion: string;
  loader: string;
  description: string;
}

export interface WorldInfo {
  name: string;
  size: string;
  lastPlayed: string | null;
  gameType: string;
  difficulty: string;
  seed: number;
  spawnX: number;
  spawnY: number;
  spawnZ: number;
  time: number;
  raining: boolean;
  thundering: boolean;
}

export interface WorldListItem {
  name: string;
  size: number;
  sizeFormatted: string;
  hasLevelDat: boolean;
  hasIcon?: boolean;
  displayName?: string | null;
  gameType?: string | null;
  difficulty?: string | null;
  hardcore?: boolean | null;
  cheatsEnabled?: boolean | null;
  /** Unix epoch milliseconds, from level.dat LastPlayed. */
  lastPlayed?: number | null;
}

export interface WorldBackupEntry {
  file: string;
  worldName: string | null;
  size: number;
  sizeFormatted: string;
  /** Unix epoch seconds (file mtime). */
  createdAt: number;
}

export interface ScreenshotEntry {
  fileName: string;
  /** Absolute path — render via convertFileSrc (asset protocol). */
  path: string;
  sizeBytes: number;
  sizeFormatted: string;
  /** Unix epoch milliseconds (file mtime). */
  modifiedMs: number;
}

export interface ContentPackEntry {
  name: string;
  fileName: string;
  enabled: boolean;
  kind: string;
  size: number;
  sizeFormatted: string;
}

export interface McServerEntry {
  name: string;
  address: string;
  icon: string | null;
  acceptTextures: number | null;
}

export interface McServerPing {
  address: string;
  online: boolean;
  latencyMs: number | null;
  error: string | null;
  playersOnline?: number | null;
  playersMax?: number | null;
}

export interface WorldDetail {
  name: string;
  seed: number;
  gameType: string | number;
  difficulty: string | number;
  /** Unix epoch milliseconds, from level.dat LastPlayed. */
  lastPlayed: number | null;
  time: number;
  spawnX: number;
  spawnY: number;
  spawnZ: number;
  hardcore: boolean;
  cheatsEnabled: boolean;
  sizeBytes: number;
  sizeFormatted: string;
}

export interface ChunkCell {
  present: number;
  lastModified: number;
  status: number;
  inhabitedTime?: number;
  dataVersion?: number;
  biomeId?: number;
  surfaceY?: number;
  entityCount?: number;
  structureCount?: number;
}

export interface RegionInfo {
  regionX: number;
  regionZ: number;
  present: number;
  minModified: number;
  maxModified: number;
  chunks: ChunkCell[];
}

export type WorldDimension = "overworld" | "nether" | "end";

export interface WorldMap {
  regions: RegionInfo[];
  minRegionX: number;
  minRegionZ: number;
  maxRegionX: number;
  maxRegionZ: number;
  totalPresent: number;
  regionCount: number;
  dimension?: WorldDimension | string;
}

export interface ChunkData {
  regionX: number;
  regionZ: number;
  index: number;
  data: number[];
  lastModified: number;
}

export interface ChunkClipboard {
  sourceWorld: string;
  chunks: ChunkData[];
  bounds: [number, number, number, number];
  entities?: ChunkData[];
  poi?: ChunkData[];
}

export interface NbtNode {
  tagType: number;
  name: string;
  value?: unknown;
  children?: NbtNode[];
  listType?: number;
}

export interface ChunkEditorData {
  regionX: number;
  regionZ: number;
  index: number;
  chunkX: number;
  chunkZ: number;
  layer: string;
  root: NbtNode;
}

export interface NbtChangeRequest {
  inhabitedTime?: number | null;
  status?: string | null;
  dataVersion?: number | null;
  lightPopulated?: number | null;
  lastUpdate?: number | null;
  biome?: string | null;
  deleteSections?: string | null;
  replaceBlocks?: string | null;
  deleteStructureRefs?: string | null;
  preventRetrogen?: boolean;
  forceBlend?: boolean;
  deleteEntities?: boolean;
  fixStatus?: boolean;
  force?: boolean;
}

export interface AdvancedChunkFilter {
  entityNames?: string | null;
  structureNames?: string | null;
  paletteNames?: string | null;
  minEntities?: number | null;
  maxEntities?: number | null;
}

export interface ChunkRef {
  regionX: number;
  regionZ: number;
  index: number;
}

export interface JavaRuntime {
  path: string;
  version: string;
  major: number;
}

export interface MinecraftVersion {
  id: string;
  type: "release" | "snapshot" | "old_beta" | "old_alpha";
}

export interface LoaderVersion {
  version: string;
  stable: boolean;
}

export interface RunningInstance {
  id: string;
  pid: number;
  profile: string;
  startedAt: number;
}

export interface LiveDebugStats {
  hostCpuPercent: number;
  hostMemoryUsedMb: number;
  hostMemoryTotalMb: number;
  instance: null | {
    pid: number;
    profile: string;
    startedAt: number;
    cpuPercent: number;
    memoryMb: number;
    virtualMemoryMb: number;
  };
}

export interface CosmeticsProfile {
  playerKey: string;
  username: string;
  skinModel: string;
  sharePublic: boolean;
  wings?: string | null;
  hat?: string | null;
  trail?: boolean;
  jumpCircles?: boolean;
  hitParticles?: boolean;
  hitBubbles?: boolean;
  targetEsp?: boolean;
  killEffect?: boolean;
  capeMeta?: Record<string, unknown>;
  writeSecret: string;
  skinPath?: string | null;
  capePath?: string | null;
}

export interface KeyboardShortcut {
  key: string;
  description: string;
  category: string;
}

export interface BackupEntry {
  id: string;
  name: string;
  createdAt: string;
  sizeBytes: number;
  fileCount: number;
}

export interface LocalizationEntry {
  key: string;
  ru: string;
}

export interface ProjectStats {
  totalLaunches: number;
  totalCrashes: number;
  totalPlaytimeSeconds: number;
  lastLaunch: string | null;
  byProfile: Array<{
    id: string;
    launches: number;
    crashes: number;
    playtimeSeconds: number;
    lastLaunch: string | null;
  }>;
}

export interface ConfigSearchMatch {
  path: string;
  line: number;
  column: number;
  lineContent: string;
}

export interface LintResult {
  severity: "error" | "warning";
  line: number;
  column: number;
  message: string;
  code: string;
}

export interface ProjectValidationResult {
  valid: boolean;
  errors: string[];
  warnings: string[];
}

export interface HealthReport {
  diagnostics: Diagnostic[];
  crashStatus: { hasCrashed: boolean; reportCount: number };
  performanceIssues: string[];
}

// ─── API wrapper ────────────────────────────────────────────────────

function pathArg(p?: string): { path: string } {
  return { path: p ?? get(projectPath) ?? "" };
}

async function cmd<T>(name: string, args?: Record<string, unknown>): Promise<T> {
  if (!isTauri()) {
    throw new Error(`Desktop IPC unavailable (${name}). Run the Tauri app, not the browser preview.`);
  }
  try {
    return await invoke<T>(name, args);
  } catch (e) {
    console.error(`[api] ${name} failed:`, e);
    throw e;
  }
}

// ─── Domain API ─────────────────────────────────────────────────────

export const api = {

  // ── Project ───────────────────────────────────────────────────────
  project: {
    validate(p?: string) { return cmd<ProjectSummary>("validate_project", pathArg(p)); },
    resolvePath(p?: string) { return cmd<string>("resolve_project_path", pathArg(p)); },
    getSchemaStatus(p?: string) { return cmd<SchemaStatus>("get_project_schema_status", pathArg(p)); },
    migrateSchema(p?: string) { return cmd<SchemaStatus>("migrate_project_schema", pathArg(p)); },
    getBrief(p?: string) { return cmd<PackBrief>("get_project_brief", pathArg(p)); },
    updateBrief(brief: PackBrief, p?: string) { return cmd<void>("update_project_brief", { ...pathArg(p), brief }); },
    getListing(p?: string) { return cmd<ProjectListing>("get_project_listing", pathArg(p)); },
    updateListing(listing: ProjectListing, p?: string) {
      return cmd<void>("update_project_listing", { ...pathArg(p), listing });
    },
    updateBriefAndListing(brief: PackBrief, listing: ProjectListing, p?: string) {
      return cmd<void>("update_project_brief_and_listing", { ...pathArg(p), brief, listing });
    },
    setListingIcon(sourceFile: string, p?: string) {
      return cmd<ProjectListing>("set_project_listing_icon", { ...pathArg(p), sourceFile });
    },
    clearListingIcon(p?: string) {
      return cmd<ProjectListing>("clear_project_listing_icon", pathArg(p));
    },
    addListingGalleryImage(opts: {
      sourceFile?: string | null;
      url?: string | null;
      caption?: string | null;
    }, p?: string) {
      return cmd<ProjectListing>("add_listing_gallery_image", {
        ...pathArg(p),
        sourceFile: opts.sourceFile ?? null,
        url: opts.url ?? null,
        caption: opts.caption ?? null,
      });
    },
    addListingGalleryBytes(bytesBase64: string, extension?: string | null, caption?: string | null, p?: string) {
      return cmd<ProjectListing>("add_listing_gallery_bytes", {
        ...pathArg(p),
        bytesBase64,
        extension: extension ?? null,
        caption: caption ?? null,
      });
    },
    removeListingGalleryImage(index: number, p?: string) {
      return cmd<ProjectListing>("remove_listing_gallery_image", { ...pathArg(p), index });
    },
    reorderListingGallery(from: number, to: number, p?: string) {
      return cmd<ProjectListing>("reorder_listing_gallery", { ...pathArg(p), from, to });
    },
    readListingAsset(relativePath: string, p?: string) {
      return cmd<string>("read_listing_asset", { ...pathArg(p), relativePath });
    },
    ensureListingFolder(p?: string) {
      return cmd<string>("ensure_listing_folder", pathArg(p));
    },
    getDir(p?: string) { return cmd<string>("get_project_dir", pathArg(p)); },
    getManifestSchema(p?: string) { return cmd<Record<string, unknown>>("get_manifest_schema", pathArg(p)); },
    runValidation(p?: string) { return cmd<Record<string, unknown>>("run_project_validation", pathArg(p)); },
    getDiagnostics(p?: string) { return cmd<Diagnostic[]>("get_diagnostics", pathArg(p)); },
    getDiagnosticCounts(p?: string) { return cmd<DiagnosticCounts>("get_diagnostic_counts", pathArg(p)); },
    getHealthReport(p?: string) {
      return cmd<{
        manifestPath: string;
        diagnostics?: { severity: string; code: string; message: string; relatedNodes?: string[] }[];
        errorCount?: number;
        warningCount?: number;
        hasCrash?: boolean;
        crashReports?: string[];
        exportBlockers?: { code: string; message: string; target?: string | null }[];
        missingFiles?: string[];
        missingHashes?: string[];
        missingCount?: number;
        hashMismatchCount?: number;
      }>("get_health_report", pathArg(p));
    },
    repair(p?: string) {
      return cmd<{
        downloaded?: string[];
        failed?: { modId: string; error: string }[];
        alreadyPresent?: string[];
        skipped?: string[];
        duplicates?: Record<string, unknown>[];
        wrongLoader?: Record<string, unknown>[];
      }>("repair_project", pathArg(p));
    },
    cleanup(p?: string) { return cmd<Record<string, unknown>>("cleanup_project", pathArg(p)); },
    listProfiles(p?: string) { return cmd<ProfileSummary[]>("list_profiles", pathArg(p)); },
  },

  // ── Cosmetics (internal — Me UI removed; launch inject + in-game mod) ──
  /** @internal Prefer in-game cosmetics menu (Right Shift). Kept for debug/inject tooling. */
  cosmetics: {
    getLocal(playerKey: string) {
      return cmd<CosmeticsProfile>("cosmetics_get_local_profile", { playerKey });
    },
    wingsCatalog() {
      return cmd<Array<{ id: string; label: string }>>("cosmetics_wings_catalog");
    },
    hatCatalog() {
      return cmd<Array<{ id: string; label: string }>>("cosmetics_hat_catalog");
    },
  },

  // ── Mods ──────────────────────────────────────────────────────────
  mods: {
    list(p?: string) { return cmd<ModInfo[]>("list_mods", pathArg(p)); },
    syncFolder(p?: string) { return cmd<Record<string, unknown>[]>("sync_mods_folder", pathArg(p)); },
    importLocal(
      sourcePaths: string[],
      contentType?: string | null,
      p?: string,
    ) {
      return cmd<{
        imported: string[];
        identified: string[];
        skipped: string[];
        baselineUpdated?: boolean;
      }>("import_local_content_files", {
        ...pathArg(p),
        sourcePaths,
        contentType: contentType ?? null,
      });
    },
    add(modId: string, side: string, p?: string) { return cmd<void>("add_modrinth_mod", { ...pathArg(p), modId, side }); },
    addWithDeps(modId: string, side: string, p?: string, dependencyTargets?: string[] | null) { return cmd<string[]>("add_modrinth_mod_with_dependencies", { ...pathArg(p), modId, side, dependencyTargets: dependencyTargets ?? null }); },
    addManyWithDeps(modIds: string[], side: string, p?: string, dependencyTargets?: string[] | null) { return cmd<string[]>("add_modrinth_mods_with_dependencies", { ...pathArg(p), modIds, side, dependencyTargets: dependencyTargets ?? null }); },
    resolveInstallDependencies(modId: string, p?: string) { return cmd<{ target: string; name: string | null; depth: number }[]>("resolve_install_dependencies", { ...pathArg(p), modId }); },
    addCurseforge(modId: string, side: string, p?: string) {
      return cmd<void>("add_curseforge_mod", { ...pathArg(p), modId, side });
    },
    addCurseforgeManyWithDeps(modIds: string[], side: string, p?: string) {
      return cmd<string[]>("add_curseforge_mods_with_dependencies", { ...pathArg(p), modIds, side });
    },
    // ── Mod presets (Optimize → user-built sets) ──
    getPresets() {
      return cmd<{ presets: Array<{ id: string; name: string; createdAt: string; mods: Array<{ provider: string; projectId: string; slug: string; name: string }> }> }>("get_mod_presets");
    },
    savePresets(store: { presets: Array<{ id: string; name: string; createdAt: string; mods: Array<{ provider: string; projectId: string; slug: string; name: string }> }> }) {
      return cmd<void>("save_mod_presets_cmd", { store });
    },
    resolvePresetMod(entry: { provider: string; projectId: string; slug?: string; name?: string }, p?: string) {
      return cmd<{
        slug: string; name: string; provider: string; projectId: string;
        versionId?: string | null; reason: string; risk: string; alreadyInstalled: boolean;
      }>("resolve_preset_mod", { ...pathArg(p), entry: { slug: entry.slug ?? "", name: entry.name ?? "", provider: entry.provider, projectId: entry.projectId } });
    },
    /** Install Steam Bridge jar matching this pack's MC + loader (GitHub releases). */
    installSteamBridge(p?: string) {
      return cmd<{
        modId: string;
        fileName: string;
        tag: string;
        mcVersion: string;
        loader: string;
        matchKind: string;
        repo: string;
      }>("install_steam_bridge", pathArg(p));
    },
    remove(modId: string, p?: string) { return cmd<void>("remove_project_mod", { ...pathArg(p), modId }); },
    update(modId: string, p?: string, versionId?: string | null) {
      return cmd<Record<string, unknown>>("update_project_mod", {
        ...pathArg(p),
        modId,
        versionId: versionId ?? null,
      });
    },
    changeVersion(modId: string, newVersionId: string, p?: string) { return cmd<Record<string, unknown>>("change_mod_version", { ...pathArg(p), modId, newVersionId }); },
    getVersions(modId: string, minecraftVersion: string, loader?: string | null) { return cmd<Record<string, unknown>[]>("get_mod_versions", { modId, minecraftVersion, loader }); },
    checkUpdates(p?: string) { return cmd<Record<string, unknown>[]>("check_mod_updates", pathArg(p)); },
    updateAll(p?: string, dryRun?: boolean) {
      return cmd<{
        dryRun?: boolean;
        count?: number;
        preview?: Record<string, unknown>[];
        updated?: string[];
        errors?: string[];
        skipped?: string[];
        download?: Record<string, unknown>;
      }>("update_all_mods", { ...pathArg(p), dryRun: dryRun ?? false });
    },
    retryFailedDownloads(modIds: string[], p?: string) {
      return cmd<Record<string, unknown>>("retry_failed_mod_downloads", { ...pathArg(p), modIds });
    },
    recommend(p?: string) { return cmd<Record<string, unknown>[]>("recommend_mods", pathArg(p)); },
    listCuratedOptimizePacks(p?: string) {
      return cmd<{
        loader: string;
        minecraftVersion: string;
        available: boolean;
        unavailableMessage?: string | null;
        current: { projectId: string; slug?: string | null; name?: string | null } | null;
        entries: Array<{ minecraftVersion: string; projectId: string; slug?: string | null; name?: string | null }>;
      }>("list_curated_optimize_packs", pathArg(p));
    },
    previewCuratedOptimizePack(p?: string) {
      return cmd<{
        pack: { projectId: string; slug: string; name: string; versionId: string; versionNumber?: string };
        mods: Array<{ slug: string; name: string; projectId: string; alreadyInstalled: boolean; role: string }>;
        configActions: Record<string, unknown>[];
        warnings: string[];
        minecraftVersion: string;
        loader: string;
      }>("preview_curated_optimize_pack", pathArg(p));
    },
    installCuratedOptimizePack(
      applyConfigs: boolean,
      configPlan?: Record<string, unknown> | null,
      p?: string,
    ) {
      return cmd<Record<string, unknown>>("install_curated_optimize_pack", {
        ...pathArg(p),
        applyConfigs,
        configPlan: configPlan ?? null,
      });
    },
    buildOptimizePlan(useAiConfigs: boolean, p?: string) {
      return cmd<{
        mode: string;
        mods: Array<{
          slug: string;
          name: string;
          provider: string;
          projectId: string;
          versionId?: string | null;
          reason: string;
          risk: string;
          alreadyInstalled: boolean;
        }>;
        plan: Record<string, unknown>;
        findings: Record<string, unknown>[];
        warnings: string[];
        minecraftVersion: string;
        loader: string;
        curatedAvailable: boolean;
        catalogSource?: string;
      }>("build_optimize_plan", { ...pathArg(p), useAiConfigs });
    },
    applyOptimizeCustomPlan(
      mods: Array<Record<string, unknown>>,
      applyConfigs: boolean,
      configPlan: Record<string, unknown> | null,
      p?: string,
    ) {
      return cmd<Record<string, unknown>>("apply_optimize_custom_plan", {
        ...pathArg(p),
        mods,
        applyConfigs,
        configPlan,
      });
    },
    previewFoOptimizePack(p?: string) {
      return cmd<{
        pack: {
          projectId: string;
          slug: string;
          name: string;
          versionId: string;
          versionNumber?: string;
          minecraftVersion: string;
          loader: string;
          modCount: number;
        };
        mods: Array<{
          slug: string;
          name: string;
          fileName?: string;
          provider: string;
          projectId: string;
          versionId?: string | null;
          reason: string;
          risk: string;
          alreadyInstalled: boolean;
        }>;
        configActions: Record<string, unknown>[];
        warnings: string[];
        minecraftVersion: string;
        loader: string;
      }>("preview_fo_optimize_pack", pathArg(p));
    },
    applyFoOptimizePlan(
      mods: Array<Record<string, unknown>>,
      applyConfigs: boolean,
      configPlan: Record<string, unknown> | null,
      p?: string,
    ) {
      return cmd<Record<string, unknown>>("install_fo_optimize_pack", {
        ...pathArg(p),
        mods,
        applyConfigs,
        configPlan,
      });
    },
    disable(modId: string, p?: string) {
      return cmd<{ id: string; disabled: boolean; fileName?: string }>("disable_project_mod", {
        ...pathArg(p),
        modId,
      });
    },
    enable(modId: string, p?: string) {
      return cmd<{ id: string; disabled: boolean; fileName?: string }>("enable_project_mod", {
        ...pathArg(p),
        modId,
      });
    },
    detectWrongLoader(p?: string) { return cmd<Record<string, unknown>[]>("detect_wrong_loader_mods", pathArg(p)); },
    disableJar(fileName: string, p?: string) { return cmd<string>("disable_wrong_loader_jar", { ...pathArg(p), fileName }); },
    removeLooseJar(fileName: string, p?: string) { return cmd<string>("remove_loose_jar", { ...pathArg(p), fileName }); },
    detectDuplicateModJars(p?: string) {
      return cmd<Array<{
        modId: string;
        keepCandidate: string;
        jars: Array<{ fileName: string; modId: string; mtimeMs: number; size: number; inManifest: boolean }>;
      }>>("detect_duplicate_mod_jars", pathArg(p));
    },
    keepOneDuplicateModJar(modId: string, keepFileName: string, p?: string) {
      return cmd<string>("keep_one_duplicate_mod_jar", { ...pathArg(p), modId, keepFileName });
    },
    checkCompatibility(p?: string) { return cmd<Record<string, unknown>[]>("check_mod_compatibility", pathArg(p)); },
    getInfo(slug: string) { return cmd<Record<string, unknown> | null>("get_mod_info", { slug }); },
    compareModpacks(pathA: string, pathB: string) { return cmd<Record<string, unknown>>("compare_modpacks", { pathA, pathB }); },

    // Modrinth / CurseForge / unified search
    search(query: string, opts?: {
      gameVersion?: string | null; loader?: string | null; category?: string | null;
      environment?: string | null; license?: string | null; sort?: string | null;
      contentType?: string | null; page?: number; pageSize?: number; p?: string;
    }) {
      const { p, ...rest } = opts ?? {};
      return cmd<{ results: SearchResult[]; total: number }>("search_modrinth_mods", {
        ...pathArg(p),
        query,
        ...rest,
      });
    },
    searchCurseforge(query: string, opts?: {
      gameVersion?: string | null; loader?: string | null; contentType?: string | null;
      categoryId?: number | null;
      page?: number; pageSize?: number; sortField?: number | null; p?: string;
    }) {
      const { p, ...rest } = opts ?? {};
      return cmd<{ results: SearchResult[]; total: number }>("search_curseforge_mods", {
        ...pathArg(p),
        query,
        ...rest,
      });
    },
    searchUnified(query: string, opts?: {
      gameVersion?: string | null; loader?: string | null; contentType?: string | null;
      page?: number; pageSize?: number; p?: string;
    }) {
      const { p, ...rest } = opts ?? {};
      return cmd<{ results: SearchResult[]; total: number }>("search_unified_mods", {
        ...pathArg(p),
        query,
        ...rest,
      });
    },
    searchContent(query: string, opts?: {
      provider?: "modrinth" | "curseforge" | "both";
      gameVersion?: string | null; loader?: string | null; contentType?: string | null;
      page?: number; pageSize?: number; p?: string;
    }) {
      const { p, ...rest } = opts ?? {};
      return cmd<{ results: SearchResult[]; total: number }>("search_content", {
        ...pathArg(p), query, ...rest,
      });
    },
    installContentBatch(contentIds: string[], provider: "modrinth" | "curseforge", side = "both", p?: string) {
      return cmd<string[]>("install_content_batch", { ...pathArg(p), contentIds, provider, side });
    },

    previewInstall(modId: string, p?: string) { return cmd<ModInstallPreview>("preview_modrinth_install", { ...pathArg(p), modId }); },
    previewCurseforgeInstall(modId: string, p?: string) {
      return cmd<ModInstallPreview>("preview_curseforge_install", { ...pathArg(p), modId });
    },
    getIcon(projectId: string) { return cmd<string | null>("get_modrinth_project_icon", { projectId }); },
    getProject(projectId: string) { return cmd<SearchResult>("get_modrinth_project", { projectId }); },
    listCategories(projectType?: string | null) {
      return cmd<Array<{ name: string; projectType: string; header: string; icon: string }>>(
        "list_modrinth_categories",
        { projectType: projectType ?? null },
      );
    },
    listCurseforgeCategories(projectType?: string | null) {
      return cmd<Array<{ id: number; name: string; parentCategoryId: number | null }>>(
        "list_curseforge_categories",
        { projectType: projectType ?? null },
      );
    },
    getUserState(p?: string) {
      return cmd<{ favorites: Record<string, boolean>; lists: Record<string, string[]>; ratings: Record<string, number> }>("get_mod_user_state", pathArg(p));
    },
    setUserState(modId: string, patch: { favorite?: boolean; saved?: boolean; rating?: number }, p?: string) {
      return cmd<{ favorites: Record<string, boolean>; lists: Record<string, string[]>; ratings: Record<string, number> }>("set_mod_user_state", {
        ...pathArg(p), modId,
        favorite: patch.favorite ?? null,
        saved: patch.saved ?? null,
        rating: patch.rating ?? null,
      });
    },
    createList(name: string, p?: string) {
      return cmd<{ favorites: Record<string, boolean>; lists: Record<string, string[]>; ratings: Record<string, number> }>("create_mod_list", { ...pathArg(p), name });
    },
    deleteList(name: string, p?: string) {
      return cmd<{ favorites: Record<string, boolean>; lists: Record<string, string[]>; ratings: Record<string, number> }>("delete_mod_list", { ...pathArg(p), name });
    },
    renameList(oldName: string, newName: string, p?: string) {
      return cmd<{ favorites: Record<string, boolean>; lists: Record<string, string[]>; ratings: Record<string, number> }>("rename_mod_list", { ...pathArg(p), oldName, newName });
    },
    addToList(name: string, modId: string, p?: string) {
      return cmd<{ favorites: Record<string, boolean>; lists: Record<string, string[]>; ratings: Record<string, number> }>("add_to_mod_list", { ...pathArg(p), name, modId });
    },
    removeFromList(name: string, modId: string, p?: string) {
      return cmd<{ favorites: Record<string, boolean>; lists: Record<string, string[]>; ratings: Record<string, number> }>("remove_from_mod_list", { ...pathArg(p), name, modId });
    },
  },

  // ── Config ────────────────────────────────────────────────────────
  config: {
    list(p?: string) { return cmd<ConfigFileSummary[]>("list_config_files", pathArg(p)); },
    read(relativePath: string, p?: string) { return cmd<string>("read_config_file", { ...pathArg(p), relativePath }); },
    write(relativePath: string, content: string, p?: string) {
      return cmd<{ snapshotId: string }>("write_config_file", { ...pathArg(p), relativePath, content });
    },
    search(query: string, p?: string) { return cmd<ConfigSearchMatch[]>("search_in_configs", { ...pathArg(p), query }); },
    lint(relativePath: string, p?: string) { return cmd<LintResult[]>("lint_config", { ...pathArg(p), relativePath }); },
    formatToml(content: string) { return cmd<string>("format_toml", { content }); },
    advise(opts: {
      goal?: string;
      userMessage?: string;
      focusPath?: string | null;
      focusKeys?: string[] | null;
      sessionId?: string | null;
      enableWebResearch?: boolean | null;
      path?: string;
    }) {
      return cmd<{
        plan: Record<string, unknown>;
        explanation: string;
        researchLog: { step: string; detail: string; ok: boolean; url?: string | null }[];
        unknownKeys: { path: string; key: string; modHint?: string | null }[];
        diffs: {
          path: string;
          patchType: string;
          beforeExcerpt: string;
          afterExcerpt: string;
          ok: boolean;
          error?: string | null;
        }[];
        validationOk: boolean;
        validationErrors: string[];
        validationWarnings: string[];
      }>("tune_config_advise", {
        ...pathArg(opts.path),
        goal: opts.goal ?? "free_text",
        userMessage: opts.userMessage ?? "",
        focusPath: opts.focusPath ?? null,
        focusKeys: opts.focusKeys ?? null,
        sessionId: opts.sessionId ?? null,
        enableWebResearch: opts.enableWebResearch ?? null,
      });
    },
    previewDiffs(plan: Record<string, unknown>, p?: string) {
      return cmd<
        {
          path: string;
          patchType: string;
          beforeExcerpt: string;
          afterExcerpt: string;
          ok: boolean;
          error?: string | null;
        }[]
      >("tune_config_preview_diffs", { ...pathArg(p), plan });
    },
    listChats(p?: string) {
      return cmd<{ sessions: TuneChatSession[]; corruptSkipped: number }>(
        "list_tune_chat_sessions",
        pathArg(p),
      );
    },
    newChat(title?: string | null, p?: string) {
      return cmd<TuneChatSession>("new_tune_chat_session", {
        ...pathArg(p),
        title: title ?? null,
      });
    },
    loadChat(chatId: string, p?: string) {
      return cmd<TuneChatSession>("load_tune_chat_session", {
        ...pathArg(p),
        chatId,
      });
    },
    saveChat(session: TuneChatSession, p?: string) {
      return cmd<void>("save_tune_chat_session", { ...pathArg(p), session });
    },
    deleteChat(chatId: string, p?: string) {
      return cmd<void>("delete_tune_chat_session", { ...pathArg(p), chatId });
    },
    chatTurn(
      message: string,
      opts?: {
        chatId?: string | null;
        goal?: string | null;
        focusPath?: string | null;
        focusKeys?: string[] | null;
        enableWebResearch?: boolean | null;
      },
      p?: string,
    ) {
      return cmd<TuneChatTurnResult>("tune_chat_turn", {
        ...pathArg(p),
        message,
        chatId: opts?.chatId ?? null,
        goal: opts?.goal ?? null,
        focusPath: opts?.focusPath ?? null,
        focusKeys: opts?.focusKeys ?? null,
        enableWebResearch: opts?.enableWebResearch ?? null,
      });
    },
  },

  // ── Graph & Resolve ───────────────────────────────────────────────
  graph: {
    get(p?: string) { return cmd<DependencyGraph>("get_graph", pathArg(p)); },
    refresh(p?: string) { return cmd<DependencyGraph>("refresh_graph", pathArg(p)); },
    getResolvePlan(p?: string) { return cmd<ChangePlan | null>("get_resolve_change_plan", pathArg(p)); },
    applyAction(actionIndex: number, p?: string) { return cmd<string[]>("apply_resolve_action", { ...pathArg(p), actionIndex }); },
    applyPlan(p?: string) { return cmd<string[]>("apply_resolve_change_plan", pathArg(p)); },
    resolveMissing(p?: string) { return cmd<string[]>("resolve_missing_dependencies", pathArg(p)); },
    exportDot(p?: string) { return cmd<string>("export_graph_dot", pathArg(p)); },
  },

  // ── Launch ────────────────────────────────────────────────────────
  launch: {
    profile(profile: string, p?: string, memoryMbOverride?: number | null) {
      return cmd<LaunchResult>("launch_profile", {
        ...pathArg(p),
        profile,
        memoryMbOverride: memoryMbOverride ?? null,
      });
    },
    server(
      serverDir: string,
      p?: string,
      opts?: { levelSeed?: string | null; onlineMode?: boolean | null },
    ) {
      return cmd<LaunchResult>("launch_server", {
        ...pathArg(p),
        serverDir,
        levelSeed: opts?.levelSeed ?? null,
        onlineMode: opts?.onlineMode ?? null,
      });
    },
    quickPlay(
      profile: string,
      quickPlayType?: string | null,
      quickPlayValue?: string | null,
      p?: string,
      memoryMbOverride?: number | null,
    ) {
      return cmd<LaunchResult>("launch_with_quick_play", {
        ...pathArg(p),
        profile,
        quickPlayType,
        quickPlayValue,
        memoryMbOverride: memoryMbOverride ?? null,
      });
    },
    listRunning() { return cmd<RunningInstance[]>("list_running_instances"); },
    kill(instanceId: string) { return cmd<string>("kill_running_instance", { instanceId }); },
    liveDebug(instanceId?: string | null) {
      return cmd<LiveDebugStats>("get_live_debug_stats", { instanceId: instanceId ?? null });
    },
    generateServerProperties(
      p?: string,
      opts?: {
        levelSeed?: string | null;
        onlineMode?: boolean | null;
        targetDir?: string | null;
      },
    ) {
      return cmd<string>("generate_server_properties", {
        ...pathArg(p),
        levelSeed: opts?.levelSeed ?? null,
        onlineMode: opts?.onlineMode ?? null,
        targetDir: opts?.targetDir ?? null,
      });
    },
  },

  // ── Stats & History ───────────────────────────────────────────────
  stats: {
    recordLaunch(p?: string) { return cmd<void>("record_launch", pathArg(p)); },
    recordCrash(p?: string) { return cmd<void>("record_crash", pathArg(p)); },
    get(p?: string) { return cmd<ProjectStats>("get_launch_stats", pathArg(p)); },
  },
  history: {
    getSettings(p?: string) { return cmd<HistorySettings>("get_history_settings", pathArg(p)); },
    updateSettings(settings: HistorySettings, p?: string) { return cmd<HistorySettings>("update_history_settings", { ...pathArg(p), settings }); },
    list(p?: string) { return cmd<HistoryListResult>("list_project_change_history", pathArg(p)); },
    getEntryDiff(entryId: string, p?: string) {
      return cmd<string>("get_history_entry_diff", { ...pathArg(p), entryId });
    },
    readFile(relativePath: string, p?: string) { return cmd<HistoryFileContent>("read_project_history_file", { ...pathArg(p), relativePath }); },
    createSnapshot(roots: string[], p?: string) { return cmd<Snapshot>("create_tracked_history_snapshot", { ...pathArg(p), roots }); },
    rollbackFile(snapshotId: string, relativePath: string, p?: string) { return cmd<void>("rollback_history_file", { ...pathArg(p), snapshotId, relativePath }); },
    scan(p?: string) { return cmd<ScanProjectChangesResult>("scan_project_changes", pathArg(p)); },
    recentEvents(limit?: number, p?: string) {
      return cmd<PackEvent[]>("list_recent_pack_events", { ...pathArg(p), limit: limit ?? 20 });
    },
    explain(eventId: string, p?: string) {
      return cmd<Record<string, unknown>>("explain_pack_change", { ...pathArg(p), eventId });
    },
    explainEpisode(episodeId: string, p?: string) {
      return cmd<Record<string, unknown>>("explain_history_episode", { ...pathArg(p), episodeId });
    },
  },

  // ── Snapshots ─────────────────────────────────────────────────────
  snapshots: {
    list(projectDir?: string) { return cmd<Snapshot[]>("list_snapshots", { projectDir: projectDir ?? get(projectPath) ?? "" }); },
    create(name: string, reason: string, projectDir?: string) { return cmd<Snapshot>("create_snapshot", { projectDir: projectDir ?? get(projectPath) ?? "", name, reason }); },
    diff(from: string, to: string, projectDir?: string) { return cmd<SnapshotDiff>("diff_snapshots", { projectDir: projectDir ?? get(projectPath) ?? "", from, to }); },
    rollback(id: string, projectDir?: string) { return cmd<Snapshot>("rollback_snapshot", { projectDir: projectDir ?? get(projectPath) ?? "", id }); },
    delete(id: string, projectDir?: string) { return cmd<void>("delete_snapshot", { projectDir: projectDir ?? get(projectPath) ?? "", id }); },
    detail(id: string, projectDir?: string) { return cmd<SnapshotDetail>("get_snapshot_detail", { projectDir: projectDir ?? get(projectPath) ?? "", id }); },
    diffManifest(fromId: string, toId: string, projectDir?: string) { return cmd<ManifestSnapshotDiff>("diff_manifest_snapshots", { projectDir: projectDir ?? get(projectPath) ?? "", fromId, toId }); },
    fileDiff(from: string, to: string, relativePath: string, projectDir?: string) { return cmd<SnapshotFileDiff>("get_snapshot_file_diff", { projectDir: projectDir ?? get(projectPath) ?? "", from, to, relativePath }); },
    diffVsCurrent(id: string, projectDir?: string) { return cmd<SnapshotVsCurrent>("diff_snapshot_vs_current", { projectDir: projectDir ?? get(projectPath) ?? "", id }); },
    fileDiffVsCurrent(id: string, relativePath: string, projectDir?: string) { return cmd<SnapshotFileDiff>("get_snapshot_file_vs_current_diff", { projectDir: projectDir ?? get(projectPath) ?? "", id, relativePath }); },
    pruneAuto(olderThanDays: number, projectDir?: string) { return cmd<PruneResult>("prune_auto_snapshots", { projectDir: projectDir ?? get(projectPath) ?? "", olderThanDays }); },
  },

  // ── Backups ───────────────────────────────────────────────────────
  backups: {
    create(name?: string | null, p?: string) { return cmd<BackupEntry>("create_project_backup", { ...pathArg(p), name }); },
    list(p?: string) { return cmd<BackupEntry[]>("list_backups", pathArg(p)); },
    delete(backupId: string, p?: string) { return cmd<void>("delete_backup", { ...pathArg(p), backupId }); },
    restore(backupId: string, p?: string) { return cmd<void>("restore_backup", { ...pathArg(p), backupId }); },
  },

  // ── Pack Diff (compare builds / snapshots / backups) ──────────────
  packDiff: {
    compare(sourceA: PackSource, sourceB: PackSource) {
      return cmd<PackStateDiff>("compare_pack_states", { sourceA, sourceB });
    },
  },

  // ── Worlds ────────────────────────────────────────────────────────
  worlds: {
    list(p?: string) { return cmd<WorldListItem[]>("list_worlds", pathArg(p)); },
    readInfo(worldName: string, p?: string) { return cmd<WorldDetail>("read_world_info", { ...pathArg(p), worldName }); },
    backup(worldName: string, p?: string) { return cmd<string>("backup_world", { ...pathArg(p), worldName }); },
    restore(backupFile: string, overwrite?: boolean, p?: string) {
      return cmd<string>("restore_world_backup", { ...pathArg(p), backupFile, overwrite: overwrite ?? false });
    },
    delete(worldName: string, backupFirst?: boolean, p?: string) {
      return cmd<void>("delete_world", { ...pathArg(p), worldName, backupFirst: backupFirst ?? false });
    },
    listBackups(p?: string) { return cmd<WorldBackupEntry[]>("list_world_backups", pathArg(p)); },
    deleteBackup(backupFile: string, p?: string) {
      return cmd<void>("delete_world_backup", { ...pathArg(p), backupFile });
    },
    readIcon(worldName: string, p?: string) {
      return cmd<string | null>("read_world_icon", { ...pathArg(p), worldName });
    },
    /** Instance screenshots (newest first) for the manager grid. */
    listScreenshots(p?: string) { return cmd<ScreenshotEntry[]>("list_screenshots", pathArg(p)); },
    deleteScreenshot(fileName: string, p?: string) {
      return cmd<void>("delete_screenshot", { ...pathArg(p), fileName });
    },
    /** Open bundled Querz MCA Selector for this world (File → Open Recent). No download. */
    openMcaSelector(worldName: string, p?: string) {
      return cmd<void>("open_mca_selector", { ...pathArg(p), worldName });
    },
    dimensions(worldName: string, p?: string) {
      return cmd<string[]>("list_world_dimensions", { ...pathArg(p), worldName });
    },
    map(worldName: string, dimension?: string, p?: string) {
      return cmd<WorldMap>("read_world_map", { ...pathArg(p), worldName, dimension: dimension ?? "overworld" });
    },
    deleteChunks(
      worldName: string,
      selections: { regionX: number; regionZ: number; indices: number[] }[],
      dimension?: string,
      p?: string,
    ) {
      return cmd<number>("delete_world_chunks", {
        ...pathArg(p),
        worldName,
        selections,
        dimension: dimension ?? "overworld",
      });
    },
    copyChunks(
      worldName: string,
      selections: { regionX: number; regionZ: number; indices: number[] }[],
      dimension?: string,
      p?: string,
    ) {
      return cmd<ChunkClipboard>("copy_world_chunks", {
        ...pathArg(p),
        worldName,
        selections,
        dimension: dimension ?? "overworld",
      });
    },
    pasteChunks(
      worldName: string,
      clipboard: ChunkClipboard,
      offsetX?: number,
      offsetZ?: number,
      dimension?: string,
      p?: string,
      overwrite?: boolean,
    ) {
      return cmd<number>("paste_world_chunks", {
        ...pathArg(p),
        worldName,
        clipboard,
        offsetX: offsetX ?? 0,
        offsetZ: offsetZ ?? 0,
        dimension: dimension ?? "overworld",
        overwrite: overwrite ?? true,
      });
    },
    purge(worldName: string, dimension?: string, p?: string) {
      return cmd<number>("purge_world_regions", {
        ...pathArg(p),
        worldName,
        dimension: dimension ?? "overworld",
      });
    },
    exportChunks(
      worldName: string,
      selections: { regionX: number; regionZ: number; indices: number[] }[],
      destDir: string,
      dimension?: string,
      p?: string,
    ) {
      return cmd<number>("export_world_chunks", {
        ...pathArg(p),
        worldName,
        selections,
        destDir,
        dimension: dimension ?? "overworld",
      });
    },
    importChunks(
      worldName: string,
      sourceDir: string,
      opts?: {
        offsetX?: number;
        offsetZ?: number;
        overwrite?: boolean;
        yOffset?: number;
        sections?: string;
        sourceSelections?: { regionX: number; regionZ: number; indices: number[] }[];
        targetSelections?: { regionX: number; regionZ: number; indices: number[] }[];
        sourceDimension?: string;
        dimension?: string;
      },
      p?: string,
    ) {
      return cmd<number>("import_world_chunks", {
        ...pathArg(p),
        worldName,
        sourceDir,
        offsetX: opts?.offsetX ?? 0,
        offsetZ: opts?.offsetZ ?? 0,
        overwrite: opts?.overwrite ?? true,
        yOffset: opts?.yOffset ?? 0,
        sections: opts?.sections,
        sourceSelections: opts?.sourceSelections ?? [],
        targetSelections: opts?.targetSelections,
        sourceDimension: opts?.sourceDimension ?? opts?.dimension ?? "overworld",
        dimension: opts?.dimension ?? "overworld",
      });
    },
    selectByQuery(worldName: string, query: string, dimension?: string, p?: string) {
      return cmd<{ regionX: number; regionZ: number; index: number }[]>("select_world_by_query", {
        ...pathArg(p),
        worldName,
        query,
        dimension: dimension ?? "overworld",
      });
    },
    renderMapPng(
      worldName: string,
      destPath: string,
      opts?: {
        colorMode?: string;
        scale?: number;
        selections?: { regionX: number; regionZ: number; indices: number[] }[];
        dimension?: string;
      },
      p?: string,
    ) {
      return cmd<[number, number]>("render_world_map_png", {
        ...pathArg(p),
        worldName,
        destPath,
        colorMode: opts?.colorMode ?? "status",
        scale: opts?.scale ?? 4,
        selections: opts?.selections ?? [],
        dimension: opts?.dimension ?? "overworld",
      });
    },
    warmCache(worldName: string, dimension?: string, p?: string) {
      return cmd<number>("warm_world_map_cache", {
        ...pathArg(p),
        worldName,
        dimension: dimension ?? "overworld",
      });
    },
    clearCache(worldName: string, dimension?: string | null, p?: string) {
      return cmd<number>("clear_world_map_cache", {
        ...pathArg(p),
        worldName,
        dimension: dimension === undefined ? "overworld" : dimension,
      });
    },
    swapChunks(
      worldName: string,
      a: { regionX: number; regionZ: number; indices: number[] },
      b: { regionX: number; regionZ: number; indices: number[] },
      dimension?: string,
      p?: string,
    ) {
      return cmd<void>("swap_world_chunks", {
        ...pathArg(p),
        worldName,
        a,
        b,
        dimension: dimension ?? "overworld",
      });
    },
    changeChunks(
      worldName: string,
      selections: { regionX: number; regionZ: number; indices: number[] }[],
      change: NbtChangeRequest,
      dimension?: string,
      p?: string,
    ) {
      return cmd<number>("change_world_chunks", {
        ...pathArg(p),
        worldName,
        selections,
        change,
        dimension: dimension ?? "overworld",
      });
    },
    readChunkEditor(
      worldName: string,
      regionX: number,
      regionZ: number,
      index: number,
      dimension?: string,
      layer?: string,
      p?: string,
    ) {
      return cmd<ChunkEditorData>("read_chunk_editor", {
        ...pathArg(p),
        worldName,
        regionX,
        regionZ,
        index,
        dimension: dimension ?? "overworld",
        layer: layer ?? "region",
      });
    },
    writeChunkEditor(
      worldName: string,
      data: ChunkEditorData,
      dimension?: string,
      p?: string,
    ) {
      return cmd<void>("write_chunk_editor", {
        ...pathArg(p),
        worldName,
        data,
        dimension: dimension ?? "overworld",
      });
    },
    filterAdvanced(
      worldName: string,
      filter: AdvancedChunkFilter,
      selections?: { regionX: number; regionZ: number; indices: number[] }[],
      dimension?: string,
      p?: string,
    ) {
      return cmd<ChunkRef[]>("filter_world_chunks_advanced", {
        ...pathArg(p),
        worldName,
        filter,
        selections: selections ?? null,
        dimension: dimension ?? "overworld",
      });
    },
  },

  // ── Recipes (JEI-style browser) ─────────────────────────────────
  recipes: {
    scan(p?: string) { return cmd<RecipeScanResult>("scan_mod_recipes", pathArg(p)); },
    itemIcon(itemId: string, p?: string) {
      return cmd<string | null>("get_item_icon", { ...pathArg(p), itemId });
    },
    itemIconsBatch(itemIds: string[], p?: string) {
      return cmd<Record<string, string | null>>("get_item_icons_batch", { ...pathArg(p), itemIds });
    },
    runtimeStatus(p?: string) { return cmd<RecipeRuntimeStatus>("get_recipe_runtime_status", pathArg(p)); },
    runtimeSnapshot(p?: string) { return cmd<RecipeRuntimeSnapshot>("get_recipe_runtime_snapshot", pathArg(p)); },
    writeRemoves(recipeIds: string[], p?: string) {
      return cmd<string>("write_kubejs_recipe_removes", { ...pathArg(p), recipeIds });
    },
    writeCraft(draft: CraftDraft, p?: string) {
      return cmd<string>("write_kubejs_craft_recipe", { ...pathArg(p), draft });
    },
    writeTags(draft: TagDraft, p?: string) {
      return cmd<string>("write_kubejs_tag_edits", { ...pathArg(p), draft });
    },
    listItemTags(p?: string) {
      return cmd<string[]>("list_item_tags", pathArg(p));
    },
    listItemCatalog(p?: string) {
      return cmd<Array<{ id: string; name: string; modNs: string }>>("list_item_catalog", pathArg(p));
    },
    getTagEntries(tagId: string, p?: string) {
      return cmd<string[]>("get_item_tag_entries", { ...pathArg(p), tagId });
    },
    generateScript(kind: string, recipeIds: string[], newItem?: string | null, count?: number | null) {
      return cmd<KubeJsScript>("generate_kubejs_recipe_script", {
        kind,
        recipeIds,
        newItem: newItem ?? null,
        count: count ?? null,
      });
    },
  },

  // ── Vanilla Minecraft client jar (recipes / quests catalog) ─────
  minecraft: {
    clientJarStatus(p?: string) {
      return cmd<VanillaClientJarStatus>("get_vanilla_client_jar_status", pathArg(p));
    },
    downloadClientJar(p?: string) {
      return cmd<string>("download_vanilla_client_jar", pathArg(p));
    },
  },

  // ── Diagnostics & Crash ───────────────────────────────────────────
  diagnostics: {
    scanOre(p?: string) { return cmd<Record<string, unknown>[]>("scan_ore_generation", pathArg(p)); },
    auditPerformance(p?: string) { return cmd<Record<string, unknown>[]>("audit_performance", pathArg(p)); },
    detectDuplicateItems(p?: string) { return cmd<Record<string, unknown>[]>("detect_duplicate_items", pathArg(p)); },
    generateUnifyConfig(save?: boolean | null, p?: string) { return cmd<Record<string, unknown>>("generate_unify_config", { ...pathArg(p), save }); },
    findClass(className: string, p?: string) { return cmd<ClassMatch[]>("find_class_in_mods", { ...pathArg(p), className }); },
    findDependents(className: string, p?: string) { return cmd<ClassMatch[]>("find_dependents_on_class", { ...pathArg(p), className }); },
    hasCrashed(p?: string) { return cmd<boolean>("has_crashed", pathArg(p)); },
    getCrashDiagnosis(reportId?: string | null, p?: string) { return cmd<CrashAnalysisReport>("get_crash_diagnosis", { ...pathArg(p), reportId }); },
    createCrashFixPlan(reportId?: string | null, p?: string) { return cmd<ChangePlan>("create_crash_fix_plan", { ...pathArg(p), reportId }); },
    applyCrashFixPlan(reportId?: string | null, p?: string) { return cmd<string[]>("apply_crash_fix_plan", { ...pathArg(p), reportId }); },
    runCrashAssistantFull(p?: string) { return cmd<CrashAnalysisReport>("run_crash_assistant_full", pathArg(p)); },
    buildAiContext(reportId?: string | null, p?: string) {
      return cmd<Record<string, unknown>>("build_ai_crash_context", {
        ...pathArg(p),
        reportId: reportId ?? null,
      });
    },
    analyzeWithAi(reportId?: string | null, p?: string) {
      return cmd<Record<string, unknown>>("analyze_crash_with_ai", {
        ...pathArg(p),
        reportId: reportId ?? null,
      });
    },
    getPackHealth(p?: string) {
      return cmd<{
        diagnostics: { errors: number; warnings: number };
        exportIssues: Array<{ severity: string; code: string; message: string }>;
        wrongLoaderCount: number;
        duplicateGroups: Array<{ modId: string; keepCandidate: string; count: number }>;
        questIssues: number;
        lastCrash: { at: string; exitCode: number | null } | null;
        overall: "healthy" | "warnings" | "errors";
      }>("get_pack_health", pathArg(p));
    },
    applyActionPlan(plan: Record<string, unknown>, p?: string, fingerprintKey?: string | null) {
      return cmd<Record<string, unknown>>("apply_action_plan", {
        ...pathArg(p),
        plan,
        fingerprintKey: fingerprintKey ?? null,
      });
    },
    recordAiFeedback(
      feedback: {
        helped: boolean;
        fingerprintKey?: string | null;
        humanExplanation?: string | null;
        suspectedMods?: string[] | null;
        recommendedActions?: Record<string, unknown>[] | null;
        reportId?: string | null;
      },
      p?: string,
    ) {
      return cmd<string>("record_crash_ai_feedback", { ...pathArg(p), feedback });
    },
    draftAuthoredCase(reportId?: string | null, p?: string) {
      return cmd<Record<string, unknown>>("draft_authored_crash_case", {
        ...pathArg(p),
        reportId: reportId ?? null,
      });
    },
    saveAuthoredCase(input: Record<string, unknown>, p?: string) {
      return cmd<Record<string, unknown>>("save_authored_crash_case", { ...pathArg(p), input });
    },
    listAuthoredCases(p?: string) {
      return cmd<Record<string, unknown>[]>("list_authored_crash_cases", pathArg(p));
    },
    getAuthoredCaseExport(caseId: string, p?: string) {
      return cmd<string>("get_authored_case_export", { ...pathArg(p), caseId });
    },
    openAuthoredKbFolder(p?: string) {
      return cmd<void>("open_authored_kb_folder", pathArg(p));
    },
    saveProblematicModsConfig(entries: Record<string, unknown>[], p?: string) { return cmd<void>("save_problematic_mods_config", { ...pathArg(p), entries }); },
    getProblematicModsConfig(p?: string) { return cmd<Record<string, unknown>[]>("get_problematic_mods_config", pathArg(p)); },
  },

  // ── Quests (FTB Quests SNBT) ─────────────────────────────────────
  quests: {
    load(p?: string) { return cmd<QuestBook>("load_quest_book", pathArg(p)); },
    /**
     * Save a chapter via `save_quest_chapter_raw`: strip locale text fields,
     * then write SNBT to the chapter file path.
     */
    async saveChapter(chapter: QuestChapter, relativePath?: string | null, p?: string) {
      const manifestOrPath = p ?? get(projectPath) ?? "";
      const projectDir = await cmd<string>("get_project_dir", pathArg(manifestOrPath));
      const rel =
        relativePath ??
        chapter.sourceFile ??
        `config/ftbquests/quests/chapters/${chapter.filename ?? chapter.id}.snbt`;
      const filePath = joinProjectPath(projectDir, rel);
      const payload = stripLocaleOverlay(chapterToSnbtJson(chapter));
      await cmd<void>("save_quest_chapter_raw", {
        filePath,
        jsonPayload: JSON.stringify(payload),
      });
      return { relativePath: rel.replace(/\\/g, "/"), questCount: chapter.quests.length };
    },
    /** Low-level: write already-prepared JSON as SNBT to an absolute path. */
    saveChapterRaw(filePath: string, jsonPayload: string) {
      return cmd<void>("save_quest_chapter_raw", { filePath, jsonPayload });
    },
    /** Same serializer as save, without writing — for SNBT preflight diff. */
    previewChapterSnbt(jsonPayload: string) {
      return cmd<string>("preview_quest_chapter_snbt", { jsonPayload });
    },
    /** Read absolute chapter path as text (disk side of preflight diff). */
    readChapterText(filePath: string) {
      return cmd<string>("read_quest_chapter_text", { filePath });
    },
    validate(p?: string) { return cmd<QuestValidationIssue[]>("validate_quest_book", pathArg(p)); },
    saveRewardTable(table: QuestRewardTable, relativePath?: string | null, p?: string) {
      return cmd<{ relativePath: string; entryCount: number }>("save_quest_reward_table", {
        ...pathArg(p),
        table,
        relativePath: relativePath ?? null,
      });
    },
    saveBookData(book: Pick<QuestBook, "title" | "subtitle" | "bookSettings">, p?: string) {
      return cmd<{ relativePath: string }>("save_quest_book_data", {
        ...pathArg(p),
        book,
      });
    },
    saveChapterGroups(groups: QuestChapterGroup[], p?: string) {
      return cmd<{ relativePath: string }>("save_quest_chapter_groups", {
        ...pathArg(p),
        groups,
      });
    },
    saveLocale(code: string, map: Record<string, string | string[]>, p?: string) {
      return cmd<{ relativePath: string }>("save_quest_locale", {
        ...pathArg(p),
        code,
        map,
      });
    },
    itemCatalog(p?: string) { return cmd<string[]>("list_quest_item_catalog", pathArg(p)); },
    listProgressTeams(p?: string) {
      return cmd<QuestProgressTeamRef[]>("list_quest_progress_teams", pathArg(p));
    },
    loadProgress(relativePath: string, p?: string) {
      return cmd<QuestProgressSnapshot>("load_quest_progress", {
        ...pathArg(p),
        relativePath,
      });
    },
    /** In-memory classify; does not touch saves/. */
    simulateProgress(
      book: QuestBook,
      completedIds: string[],
      taskProgressIds?: string[],
    ) {
      return cmd<QuestProgressSnapshot>("simulate_quest_progress", {
        book,
        completedIds,
        taskProgressIds: taskProgressIds ?? [],
      });
    },
    /** Parse AI QuestPlan JSON and merge into current book (memory only). */
    parseAndMergePlan(raw: string, p?: string) {
      return cmd<QuestPlanMergeResult>("parse_and_merge_quest_plan", {
        ...pathArg(p),
        raw,
      });
    },
    /** Natural language → QuestPlan merge preview (heuristic or AI). */
    generateFromPrompt(prompt: string, forceAi = false, p?: string) {
      return cmd<QuestPlanMergeResult>("generate_quest_plan_from_prompt", {
        ...pathArg(p),
        prompt,
        forceAi,
      });
    },
    /** Multi-pass outline → lore → ground → layout (20+ quest lines). */
    generateQuestLine(prompt: string, forceAi = false, p?: string) {
      return cmd<QuestPlanMergeResult>("generate_quest_line", {
        ...pathArg(p),
        prompt,
        forceAi,
      });
    },
    filterAndMergePlan(
      plan: QuestPlan,
      chapterKeys: string[],
      questKeys: string[],
      p?: string,
    ) {
      return cmd<QuestPlanMergeResult>("filter_and_merge_quest_plan", {
        ...pathArg(p),
        plan,
        chapterKeys,
        questKeys,
      });
    },
    listChats(p?: string) {
      return cmd<{ sessions: QuestChatSession[]; corruptSkipped: number }>(
        "list_quest_chat_sessions",
        pathArg(p),
      );
    },
    newChat(title?: string | null, p?: string) {
      return cmd<QuestChatSession>("new_quest_chat_session", {
        ...pathArg(p),
        title: title ?? null,
      });
    },
    loadChat(chatId: string, p?: string) {
      return cmd<QuestChatSession>("load_quest_chat_session", {
        ...pathArg(p),
        chatId,
      });
    },
    saveChat(session: QuestChatSession, p?: string) {
      return cmd<void>("save_quest_chat_session", { ...pathArg(p), session });
    },
    deleteChat(chatId: string, p?: string) {
      return cmd<void>("delete_quest_chat_session", { ...pathArg(p), chatId });
    },
    chatTurn(
      message: string,
      opts?: {
        chatId?: string | null;
        forceAi?: boolean;
        intent?: string | null;
        anchorQuestId?: string | null;
        targetChapterId?: string | null;
      },
      p?: string,
    ) {
      return cmd<QuestChatTurnResult>("quest_chat_turn", {
        ...pathArg(p),
        message,
        chatId: opts?.chatId ?? null,
        forceAi: opts?.forceAi ?? false,
        intent: opts?.intent ?? null,
        anchorQuestId: opts?.anchorQuestId ?? null,
        targetChapterId: opts?.targetChapterId ?? null,
      });
    },
    cancelChatTurn() {
      return cmd<void>("cancel_quest_chat_turn");
    },
    validatePlan(plan: QuestPlan) {
      return cmd<QuestPlanValidation>("validate_quest_plan", { plan });
    },
    planSystemPrompt() {
      return cmd<string>("quest_plan_system_prompt");
    },
    kubejs: {
      listScripts(p?: string) {
        return cmd<QuestKubeJsScriptFile[]>("quest_kubejs_list_scripts", pathArg(p));
      },
      audit(book: QuestBook, p?: string) {
        return cmd<QuestKubeJsAudit>("quest_kubejs_audit", { ...pathArg(p), book });
      },
      readScript(relativePath: string, p?: string) {
        return cmd<string>("quest_kubejs_read_script", { ...pathArg(p), relativePath });
      },
      ensureManaged(p?: string) {
        return cmd<string>("quest_kubejs_ensure_managed", pathArg(p));
      },
      renderTemplate(params: QuestKubeJsTemplateParams) {
        return cmd<string>("quest_kubejs_render_template", { params });
      },
      appendHandler(snippet: string, p?: string) {
        return cmd<{ relativePath: string; snapshotId: string }>("quest_kubejs_append_handler", {
          ...pathArg(p),
          snippet,
        });
      },
      writeScript(relativePath: string, content: string, p?: string) {
        return cmd<{ snapshotId: string }>("write_config_file", {
          ...pathArg(p),
          relativePath,
          content,
        });
      },
    },
  },

  // ── Export ────────────────────────────────────────────────────────
  export: {
    modrinthPack(targetPath?: string | null, p?: string) {
      return cmd<ExportResult>("export_modrinth_pack", {
        ...pathArg(p),
        targetPath: targetPath ?? null,
      });
    },
    serverPack(targetPath?: string | null, p?: string) {
      return cmd<ExportResult>("export_server_pack", {
        ...pathArg(p),
        targetPath: targetPath ?? null,
      });
    },
    prismInstance(targetPath?: string | null, p?: string) {
      return cmd<ExportResult>("export_prism_instance", {
        ...pathArg(p),
        targetPath: targetPath ?? null,
      });
    },
    curseforgePack(targetPath?: string | null, p?: string) {
      return cmd<ExportResult>("export_curseforge_pack", {
        ...pathArg(p),
        targetPath: targetPath ?? null,
      });
    },
    packwizPack(targetPath?: string | null, p?: string) {
      return cmd<ExportResult>("export_packwiz_pack", {
        ...pathArg(p),
        targetPath: targetPath ?? null,
      });
    },
    batchAll(p?: string) {
      return cmd<
        Array<{
          kind: string;
          status: string;
          path?: string;
          files?: number;
          overrideCount?: number;
          error?: string;
        }>
      >("batch_export_all", pathArg(p));
    },
    projectReport(p?: string) {
      return cmd<Record<string, unknown>>("export_project_report", pathArg(p));
    },
    validateModrinth(p?: string) {
      return cmd<ExportIssue[]>("validate_modrinth_export", pathArg(p));
    },
    validateCurseforge(p?: string) {
      return cmd<ExportIssue[]>("validate_curseforge_export", pathArg(p));
    },
    reveal(path: string) {
      return cmd<void>("reveal_export_path", { path });
    },
  },

  // ── GitHub Pack Transport (public repos, anonymous consume) ─────
  transport: {
    github: {
      parseSource(source: string) {
        return cmd<{ owner: string; repo: string; ref: string | null }>("github_pack_parse_source", { source });
      },
      inspectSource(source: string) {
        return cmd<{
          owner: string;
          repo: string;
          fullName: string;
          description: string;
          defaultBranch: string;
          htmlUrl: string;
          packVersion?: string | null;
          status?: string | null;
          ready: boolean;
          /** Install-preview metadata below: present only when the repo
           * carries .tuffbox/repo-transport.json + manifest; null otherwise. */
          projectName?: string | null;
          projectVersion?: string | null;
          mcVersion?: string | null;
          loaderKind?: string | null;
          loaderVersion?: string | null;
          modCount?: number | null;
          /** Sum of transport releaseAssets sizes; null when any is unknown. */
          totalAssetBytes?: number | null;
          /** Mods whose source.type === "local" (custom in-repo mods). */
          customModCount?: number | null;
        }>("github_pack_inspect_source", { source });
      },
      authStatus() {
        return cmd<boolean>("github_pack_auth_status");
      },
      startDeviceCode() {
        return cmd<{
          userCode: string;
          verificationUri: string;
          message: string;
          expiresIn: number;
          interval: number;
        }>("github_pack_start_device_code");
      },
      pollDeviceCode() {
        return cmd<string>("github_pack_poll_device_code");
      },
      stagePreview(p?: string) {
        return cmd<{
          packVersion: string;
          manifestFile: string;
          fileCount: number;
          managedFiles: string[];
          shareUrl: string | null;
          contentDigest: string;
          hasExternalAssets: boolean;
        }>("github_pack_stage_preview", pathArg(p));
      },
      publish(repository: string, p?: string) {
        return cmd<Record<string, unknown>>("github_pack_publish", { ...pathArg(p), repository });
      },
      install(source: string, targetDir: string, instanceName?: string | null) {
        return cmd<Record<string, unknown>>("github_pack_install", {
          source,
          targetDir,
          instanceName: instanceName ?? null,
        });
      },
      checkUpdate(p?: string) {
        return cmd<{
          updateAvailable: boolean;
          repo?: string;
          installedVersion?: string;
          installedCommitSha?: string | null;
          remoteCommitSha?: string;
          reason?: string;
        }>("github_pack_check_update", pathArg(p));
      },
      previewUpdate(p?: string) {
        return cmd<{
          repo: string;
          installedVersion: string;
          incomingVersion: string;
          remoteCommitSha: string;
          requiresFullReinstall: boolean;
          customFiles: boolean;
          signerState?: "ok" | "changed" | "unsigned";
          changes: Array<Record<string, unknown>>;
        }>("github_pack_preview_update", pathArg(p));
      },
      applyUpdate(expectedCommitSha: string, p?: string) {
        return cmd<{
          ok: boolean;
          snapshotId: string;
          version: string;
          changes: Array<Record<string, unknown>>;
        }>("github_pack_apply_update", {
          ...pathArg(p),
          expectedCommitSha,
        });
      },
    },
  },

  // ── Modpack library (remote browse + import) ─────────────────────
  modpacks: {
    getModpackUrl(projectId: string) { return cmd<string>("get_modrinth_pack_download", { projectId }); },
    install(url: string, targetDir: string, instanceName: string, dedup?: boolean | null) {
      return cmd<{
        path: string;
        download?: Record<string, unknown>;
        dedup?: Record<string, unknown>;
      }>("install_modpack", {
        source: url,
        targetDir,
        instanceName,
        dedup: dedup ?? null,
      });
    },
  },

  // ── Personal YouTube feed (own channels via the RSS hub) ──────────
  youtubeMyFeed: {
    lookup(query: string) {
      return cmd<{ channelId: string; label: string }>("youtube_my_feed_lookup", { query });
    },
    fetch(channelIds: string[]) {
      return cmd<{
        videos: {
          videoId: string;
          title: string;
          thumbnailUrl: string | null;
          channelName: string | null;
          viewCount: number | null;
          publishedAt: string | null;
        }[];
        errors: string[];
      }>("youtube_my_feed_fetch", { channelIds });
    },
  },

  // ── Release ───────────────────────────────────────────────────────
  release: {
    generateChangelog(p?: string) { return cmd<string>("generate_release_changelog", pathArg(p)); },
    generateGitHubRelease(tag?: string | null, target?: string | null, p?: string) { return cmd<Record<string, unknown>>("generate_github_release", { ...pathArg(p), tag, target }); },
    updateVersion(version: string, p?: string) { return cmd<ProjectSummary>("update_project_version", { ...pathArg(p), version }); },
    createSnapshot(changelog: string, p?: string) { return cmd<ReleaseSnapshotResult>("create_release_snapshot", { ...pathArg(p), changelog }); },
    listArtifacts(p?: string) { return cmd<ReleaseArtifactRecord[]>("list_release_artifacts", pathArg(p)); },
    createDraft(changelog: string, p?: string) { return cmd<ReleaseDraftResult>("create_release_draft", { ...pathArg(p), changelog }); },
    generateLockfile(p?: string) { return cmd<TuffboxLockfile>("generate_lockfile", pathArg(p)); },
  },

  // ── Import ────────────────────────────────────────────────────────
  import: {
    project(source: string, targetDir: string) { return cmd<string>("import_project", { source, targetDir }); },
    curseforge(source: string, targetDir: string) { return cmd<string>("import_curseforge_project", { source, targetDir }); },
    installModpack(source: string, targetDir: string, instanceName?: string | null) {
      return cmd<Record<string, unknown>>("install_modpack", { source, targetDir, instanceName: instanceName ?? null });
    },
  },

  curseforge: {
    searchModpacks(query: string, gameVersion?: string | null, offset?: number) {
      return cmd<Record<string, unknown>[]>("search_curseforge_modpacks", {
        query,
        gameVersion: gameVersion ?? null,
        offset: offset ?? 0,
      });
    },
    getModpackFiles(modId: number, gameVersion?: string | null) {
      return cmd<Record<string, unknown>[]>("get_curseforge_modpack_files", {
        modId,
        gameVersion: gameVersion ?? null,
      });
    },
  },

  // ── Content packs (resourcepacks / shaderpacks on disk) ───────────
  content: {
    listPacks(folder: "resourcepacks" | "shaderpacks", p?: string) {
      return cmd<ContentPackEntry[]>("list_content_packs", { ...pathArg(p), folder });
    },
    setEnabled(folder: "resourcepacks" | "shaderpacks", fileName: string, enabled: boolean, p?: string) {
      return cmd<ContentPackEntry>("set_content_pack_enabled", {
        ...pathArg(p),
        folder,
        fileName,
        enabled,
      });
    },
  },

  // ── Minecraft servers.dat ─────────────────────────────────────────
  servers: {
    list(p?: string) {
      return cmd<McServerEntry[]>("list_mc_servers", pathArg(p));
    },
    add(name: string, address: string, p?: string) {
      return cmd<McServerEntry[]>("add_mc_server", { ...pathArg(p), name, address });
    },
    remove(address: string, p?: string) {
      return cmd<McServerEntry[]>("remove_mc_server", { ...pathArg(p), address });
    },
    ping(address: string) {
      return cmd<McServerPing>("ping_mc_server", { address });
    },
  },

  // ── Instance ──────────────────────────────────────────────────────
  instance: {
    create(name: string, minecraftVersion: string, loader: string, loaderVersion: string, location: string) {
      return cmd<string>("create_instance", { name, minecraftVersion, loader, loaderVersion, location });
    },
    updateSettings(opts: {
      minecraftVersion: string; loader: string; loaderVersion: string;
      javaPath?: string | null; memoryMb: number; jvmArgs: string[]; playerName?: string | null;
      p?: string;
    }) { return cmd<void>("update_project_settings", { ...pathArg(opts.p), ...opts }); },
    getSize(p?: string) { return cmd<string>("get_instance_size", pathArg(p)); },
  },

  // ── Logs ──────────────────────────────────────────────────────────
  logs: {
    getLaunch(p?: string) { return cmd<string>("get_launch_log", pathArg(p)); },
    listInstance(p?: string) { return cmd<Record<string, unknown>[]>("list_instance_logs", pathArg(p)); },
    readInstance(logName: string, p?: string) { return cmd<string>("read_instance_log", { ...pathArg(p), logName }); },
    createZip(p?: string) { return cmd<string>("create_logs_zip", pathArg(p)); },
    captureTestRun(runId: string, p?: string) { return cmd<string>("capture_test_run_logs", { ...pathArg(p), runId }); },
  },

  // ── Test Runs ─────────────────────────────────────────────────────
  testRuns: {
    list(p?: string) { return cmd<TestRunRecord[]>("list_test_runs", pathArg(p)); },
    finalize(
      runId: string,
      status: string,
      opts?: {
        durationSeconds?: number | null;
        verdictReason?: string | null;
        peakProcMb?: number | null;
        peakHostMb?: number | null;
        hostTotalMb?: number | null;
        xmxMb?: number | null;
      },
      p?: string,
    ) {
      return cmd<TestRunRecord>("finalize_test_run", {
        ...pathArg(p),
        runId,
        status,
        durationSeconds: opts?.durationSeconds ?? null,
        verdictReason: opts?.verdictReason ?? null,
        peakProcMb: opts?.peakProcMb ?? null,
        peakHostMb: opts?.peakHostMb ?? null,
        hostTotalMb: opts?.hostTotalMb ?? null,
        xmxMb: opts?.xmxMb ?? null,
      });
    },
  },

  // ── Templates ─────────────────────────────────────────────────────
  templates: {
    save(templateName: string, p?: string) { return cmd<void>("save_as_template", { ...pathArg(p), templateName }); },
    list(p?: string) { return cmd<TemplateInfo[]>("list_templates", pathArg(p)); },
  },

  // ── System ────────────────────────────────────────────────────────
  system: {
    getAppVersion() { return cmd<string>("get_app_version"); },
    checkForUpdate() { return cmd<Record<string, unknown>>("check_for_app_update"); },
    getHomeDir() { return cmd<string>("get_home_dir"); },
    getMinecraftVersions() { return cmd<MinecraftVersion[]>("get_minecraft_versions"); },
    getLoaderVersions(loader: string, minecraftVersion: string) { return cmd<LoaderVersion[]>("get_loader_versions", { loader, minecraftVersion }); },
    findJavaRuntimes() { return cmd<JavaRuntime[]>("find_java_runtimes"); },
    /** Download latest GraalVM Community JDK if no Java is found (or return existing). */
    ensureJavaRuntime() { return cmd<JavaRuntime>("ensure_java_runtime"); },
    getJavaVersion(path: string) { return cmd<string>("get_java_version", { path }); },
    getDefaultJavaVersion() { return cmd<string>("get_default_java_version"); },
    getKeyboardShortcuts() { return cmd<KeyboardShortcut[]>("get_keyboard_shortcuts"); },
    getDownloadProgress() { return cmd<Record<string, unknown>[]>("get_download_progress"); },
  },

  // ── Pinning & Session ─────────────────────────────────────────────
  session: {
    pin(pin: boolean, p?: string) { return cmd<void>("pin_project", { ...pathArg(p), pin }); },
    isPinned(p?: string) { return cmd<boolean>("is_project_pinned", pathArg(p)); },
    setLastOpened(p?: string) { return cmd<void>("set_last_opened_project", pathArg(p)); },
    getLastOpened() { return cmd<string | null>("get_last_opened_project"); },
    loadRecentProjects() {
      return cmd<Array<{ path: string; info: object }>>("load_recent_projects");
    },
    saveRecentProjects(projects: Array<{ path: string; info: object }>) {
      return cmd<void>("save_recent_projects", { projects });
    },
  },

  // ── Home bootstrap pipeline ───────────────────────────────────────
  home: {
    bootstrap(selectedPath?: string | null) {
      return cmd<import("./homeBootstrap").HomeSnapshot>("get_home_bootstrap", {
        request: { selectedPath: selectedPath ?? null },
      });
    },
    projectBriefs(paths: string[]) {
      return cmd<
        Array<{
          path: string;
          stats: { playtime: number; lastLaunch: string | null };
          sizeLabel?: string | null;
          iconDataUrl?: string | null;
        }>
      >("get_home_project_briefs", { paths });
    },
    accountSkinPaths(uuids: string[]) {
      return cmd<Record<string, string>>("get_account_skin_paths", { uuids });
    },
    invalidateCache(p?: string) {
      return cmd<void>("invalidate_home_project_cache", pathArg(p));
    },
  },

  // ── File Operations ───────────────────────────────────────────────
  files: {
    openFolder(p?: string, subdir?: string | null) {
      return cmd<void>("open_project_folder", { ...pathArg(p), subdir: subdir ?? null });
    },
    deleteProject(p?: string) { return cmd<void>("delete_project", pathArg(p)); },
    cloneProject(newName: string, p?: string) { return cmd<string>("clone_project", { ...pathArg(p), newName }); },
    /** Renames the instance (manifest display name; folder path is unchanged). */
    rename(newName: string, p?: string) { return cmd<string>("rename_project", { ...pathArg(p), newName }); },
    createDesktopShortcut(p?: string) {
      return cmd<string>("create_project_desktop_shortcut", pathArg(p));
    },
    /** One-shot path from process `--launch` / `--open` (desktop shortcut). */
    takePendingLaunch() {
      return cmd<string | null>("take_pending_launch_project");
    },
    /** One-shot verdict from a `tuffbox://install?repo=…` deep link. */
    takePendingInstall() {
      return cmd<InstallLinkPayload | null>("take_pending_install_repo");
    },
  },

  // ── Localization ──────────────────────────────────────────────────
  l10n: {
    get(key: string) { return cmd<string>("localize", { key }); },
    list() { return cmd<LocalizationEntry[]>("list_localizations"); },
  },

  // ── Minecraft Auth ───────────────────────────────────────────────
  mcAuth: {
    startDeviceCode() { return cmd<DeviceCodeInfo>("mc_start_device_code"); },
    pollDeviceCode() { return cmd<{ profile: McProfile }>("mc_poll_device_code"); },
    getMicrosoftLoginUrl() { return cmd<string>("mc_get_microsoft_login_url"); },
    loginWithAuthUrl(urlOrCode: string) {
      return cmd<{ profile: McProfile }>("mc_login_with_auth_url", { urlOrCode });
    },
    startMicrosoftWebviewAuth() {
      return cmd<{ profile: McProfile }>("mc_start_microsoft_webview_auth");
    },
    offlineLogin(username: string, skinSource: SkinSource) {
      return cmd<{ profile: McProfile }>("mc_offline_login", { username, skinSource });
    },
    getAuthStatus() { return cmd<AuthState>("mc_get_auth_status"); },
    logout() { return cmd<AuthState>("mc_logout"); },
    refreshProfile() { return cmd<McProfile>("mc_refresh_profile"); },
    refreshToken() { return cmd<AuthState>("mc_refresh_token"); },
    getSkinPath(uuid: string) { return cmd<string>("mc_get_skin_path", { uuid }); },
    fetchSkinUrl(uuid: string) { return cmd<string | null>("mc_fetch_skin_url", { uuid }); },
    fetchSkinForUsername(username: string, source: SkinSource) {
      return cmd<string | null>("mc_fetch_skin_for_username", { username, source });
    },
    setSkinSource(source: SkinSource) { return cmd<void>("mc_set_skin_source", { source }); },
    listAccounts() { return cmd<AccountEntry[]>("mc_list_accounts"); },
    switchAccount(uuid: string) { return cmd<AuthState>("mc_switch_account", { uuid }); },
    removeAccount(uuid: string) { return cmd<AuthState>("mc_remove_account", { uuid }); },
    applySkin(skinUrl: string, variant: string) { return cmd<AuthState>("mc_apply_skin", { skinUrl, variant }); },
    uploadSkin(pngBase64: string, variant: string) { return cmd<AuthState>("mc_upload_skin", { pngBase64, variant }); },
    uploadSkinFile(path: string, variant: string) { return cmd<AuthState>("mc_upload_skin_file", { path, variant }); },
    applyCape(capeId: string) { return cmd<AuthState>("mc_apply_cape", { capeId }); },
    listCapes() { return cmd<CapeCatalog>("mc_list_capes"); },
    setCapeProvider(provider: CapeProvider) { return cmd<AuthState>("mc_set_cape_provider", { provider }); },
    checkEntitlement() { return cmd<boolean>("mc_check_entitlement"); },
    getSkinBase64(url: string) { return cmd<string>("mc_get_skin_base64", { url }); },
    listYggdrasilPresets() { return cmd<YggdrasilPreset[]>("mc_list_yggdrasil_presets"); },
    yggdrasilLogin(username: string, password: string, authority: string) {
      return cmd<{ profile: McProfile }>("mc_yggdrasil_login", {
        username,
        password,
        authority,
      });
    },
  },

  // ── Discord Rich Presence ─────────────────────────────────────────
  presence: {
    get() { return cmd<PresenceSettings>("get_presence_settings"); },
    save(settings: PresenceSettings) { return cmd<void>("save_presence_settings", { settings }); },
    setPlaying(details: string, state: string) {
      return cmd<void>("set_discord_presence", { details, state });
    },
    clear() { return cmd<void>("clear_discord_presence"); },
    online() { return cmd<Record<string, unknown>>("get_launcher_online"); },
    recentSessions(limit = 50) {
      return cmd<unknown[]>("get_launcher_recent_sessions", { limit });
    },
    start() { return cmd<Record<string, unknown>>("launcher_presence_start"); },
    stop() { return cmd<Record<string, unknown>>("launcher_presence_stop"); },
  },

  launcher: {
    get() { return cmd<LauncherSettings>("get_launcher_settings"); },
    save(settings: LauncherSettings) {
      return cmd<LauncherSettings>("save_launcher_settings_cmd", { settings });
    },
    detectGpus() { return cmd<GpuInfo[]>("detect_gpus"); },
    runtimePathInfo() {
      return cmd<{ current: string; default: string }>("get_runtime_path_info");
    },
    instancesPathInfo() {
      return cmd<{ current: string; default: string }>("get_instances_path_info");
    },
    validateRuntimePath(path: string) {
      return cmd<boolean>("validate_runtime_path_cmd", { path });
    },
    validateInstancesPath(path: string) {
      return cmd<boolean>("validate_instances_path_cmd", { path });
    },
  },
};

export interface GpuInfo {
  id: string;
  name: string;
  vendor: string;
  kind: string;
  vramMb: number | null;
  primary: boolean;
  pciSlot: string | null;
}

// ─── AI / Ollama settings ───────────────────────────────────────────

export type AiProvider = "ollama" | "openai-compatible";

export interface AiSettings {
  provider: string;
  endpoint: string;
  model: string;
  diagnoseMode?: string;
  crashKbEndpoint?: string;
  ollamaBinaryPath?: string;
  ollamaModelsPath?: string;
  speculativeDecoding?: boolean;
  draftModel?: string;
  /** Tune Config Advisor: allowlisted web research for unknown keys (default true). */
  tuneWebResearch?: boolean;
}

export interface OllamaModelInfo {
  name: string;
  sizeBytes: number;
  parameterSize: string;
  quantization: string;
  family: string;
  fit: string;
}

export interface SuggestedModel {
  name: string;
  note: string;
  approxSizeBytes: number;
  minRamGb: number;
  minVramGb: number;
}

export interface OllamaDetect {
  installed: boolean;
  running: boolean;
  binaryPath: string;
  modelsPath?: string;
  modelsPathConfigured?: boolean;
  defaultModel?: string;
  endpoint: string;
  models: OllamaModelInfo[];
  needsModel: boolean;
  error?: string | null;
  suggestedModels: SuggestedModel[];
  hostRamBytes?: number;
}

export interface OllamaStorage {
  path: string;
  files: number;
  usedBytes: number;
  availableBytes: number;
  totalBytes: number;
  hostRamBytes?: number;
}

export interface OllamaPullProgress {
  model: string;
  status: string;
  completed: number;
  total: number;
}

export function formatBytes(bytes: number | null | undefined): string {
  const n = Number(bytes) || 0;
  if (n <= 0) return "—";
  const units = ["B", "KB", "MB", "GB", "TB"];
  let v = n;
  let i = 0;
  while (v >= 1024 && i < units.length - 1) {
    v /= 1024;
    i += 1;
  }
  const digits = i === 0 ? 0 : v >= 10 ? 1 : 2;
  return `${v.toFixed(digits)} ${units[i]}`;
}

export function fitLabel(fit: string | undefined): string {
  switch ((fit || "").toLowerCase()) {
    case "ok":
      return "OK";
    case "tight":
      return "Tight";
    case "heavy":
      return "Heavy";
    default:
      return "—";
  }
}

/**
 * Pack metadata line for the GitHub import confirm dialog, e.g.
 * `MC 1.20.1 · fabric 0.15.11 · 142 mods (+12 custom)`.
 * Absent/null fields are omitted instead of rendering "null".
 */
export function githubInspectMeta(info: {
  mcVersion?: string | null;
  loaderKind?: string | null;
  loaderVersion?: string | null;
  modCount?: number | null;
  customModCount?: number | null;
}): string {
  const parts: string[] = [];
  if (info.mcVersion) parts.push(`MC ${info.mcVersion}`);
  const loader = [info.loaderKind, info.loaderVersion].filter(Boolean).join(" ");
  if (loader) parts.push(loader);
  if (typeof info.modCount === "number" && info.modCount > 0) {
    const custom =
      typeof info.customModCount === "number" && info.customModCount > 0
        ? ` (+${info.customModCount} custom)`
        : "";
    parts.push(`${info.modCount} ${info.modCount === 1 ? "mod" : "mods"}${custom}`);
  }
  return parts.join(" · ");
}

// ── tuffbox://install deep link ──────────────────────────────────────

export type InstallLinkPayload =
  | { status: "valid"; repo: string }
  | { status: "invalid"; raw: string };

type InstallLinkHandler = (link: InstallLinkPayload) => void;

const installLinkHandlers = new Set<InstallLinkHandler>();
let queuedInstallLink: InstallLinkPayload | null = null;

function deliverInstallLink(link: InstallLinkPayload): void {
  // The Library view owns the GitHub confirm dialog; surface it first so a
  // subscriber exists by the time the user looks.
  window.dispatchEvent(new CustomEvent("tuffbox:open-library"));
  if (!installLinkHandlers.size) {
    // Cold start: Library has not mounted yet — hold until it subscribes.
    queuedInstallLink = link;
    return;
  }
  queuedInstallLink = null;
  for (const handler of [...installLinkHandlers]) handler(link);
}

/** Subscribe to `tuffbox://install?repo=…` links. A subscriber that mounts
 * late still receives a link that arrived before it existed. Returns an
 * unlisten function. */
export function onInstallLink(handler: InstallLinkHandler): () => void {
  installLinkHandlers.add(handler);
  const queued = queuedInstallLink;
  queuedInstallLink = null;
  if (queued) handler(queued);
  return () => {
    installLinkHandlers.delete(handler);
  };
}

if (isTauri()) {
  // Cold start: drain what the backend parked before any UI existed.
  void api.files
    .takePendingInstall()
    .then((pending) => {
      if (pending) deliverInstallLink(pending);
    })
    .catch(() => {});
  // Warm start: a second instance forwarded the URL to this process.
  void listen<InstallLinkPayload | null>("tuffbox:install-link", (event) => {
    if (!event.payload) return;
    // The backend parks the same link for the next cold start; drop it so a
    // later launch doesn't replay a link the user already handled.
    void api.files.takePendingInstall().catch(() => {});
    deliverInstallLink(event.payload);
  });
}
