/** Human-readable FTB Quests task / reward type labels. */

export type QuestTypeOption = { id: string; label: string };

/** Built-in FTB Quests task types (+ common platform energy variants). */
export const TASK_TYPE_OPTIONS: readonly QuestTypeOption[] = [
  { id: "item", label: "Item (inventory / submit)" },
  { id: "observation", label: "Observe block / entity" },
  { id: "checkmark", label: "Checkmark" },
  { id: "kill", label: "Kill entity" },
  { id: "dimension", label: "Visit dimension" },
  { id: "biome", label: "Visit biome" },
  { id: "xp", label: "Experience" },
  { id: "advancement", label: "Advancement" },
  { id: "stat", label: "Statistic" },
  { id: "gamestage", label: "Game stage" },
  { id: "fluid", label: "Fluid" },
  { id: "location", label: "Location" },
  { id: "structure", label: "Structure" },
  { id: "forge_energy", label: "Forge energy (FE/RF)" },
  { id: "techreborn_energy", label: "Tech Reborn energy" },
  { id: "custom", label: "Custom" },
] as const;

/** Built-in FTB Quests reward types. */
export const REWARD_TYPE_OPTIONS: readonly QuestTypeOption[] = [
  { id: "item", label: "Item" },
  { id: "xp", label: "Experience points" },
  { id: "xp_levels", label: "Experience levels" },
  { id: "command", label: "Command" },
  { id: "random", label: "Random table" },
  { id: "choice", label: "Choice table" },
  { id: "loot", label: "Loot crate" },
  { id: "all_table", label: "All reward tables" },
  { id: "gamestage", label: "Game stage" },
  { id: "toast", label: "Toast" },
  { id: "advancement", label: "Advancement" },
  { id: "currency", label: "Currency" },
  { id: "custom", label: "Custom" },
] as const;

export const TASK_TYPES: readonly string[] = TASK_TYPE_OPTIONS.map((o) => o.id);
export const REWARD_TYPES: readonly string[] = REWARD_TYPE_OPTIONS.map((o) => o.id);

/** Observation task `observation_type` values (FTB ObserveType). */
export const OBSERVATION_TYPE_OPTIONS: readonly QuestTypeOption[] = [
  { id: "block", label: "Block" },
  { id: "block_tag", label: "Block tag" },
  { id: "block_state", label: "Block state" },
  { id: "block_entity", label: "Block entity (state+NBT)" },
  { id: "block_entity_type", label: "Block entity type" },
  { id: "entity_type", label: "Entity type" },
  { id: "entity_type_tag", label: "Entity type tag" },
] as const;

/** Item task completion modes (maps to FTB ItemTask flags). */
export const ITEM_COMPLETE_OPTIONS: readonly QuestTypeOption[] = [
  { id: "inventory", label: "Detect in inventory" },
  { id: "consume", label: "Submit / consume items" },
  { id: "craft", label: "Only from crafting" },
  { id: "task_screen", label: "Task screen only" },
] as const;

const TASK_LABELS: Record<string, string> = {
  ...Object.fromEntries(TASK_TYPE_OPTIONS.map((o) => [o.id, o.label])),
  // Legacy aliases from older editors / SNBT
  stage: "Game stage",
  energy: "Energy",
};

const REWARD_LABELS: Record<string, string> = {
  ...Object.fromEntries(REWARD_TYPE_OPTIONS.map((o) => [o.id, o.label])),
  stage: "Game stage",
  all_tables: "All reward tables",
};

export function taskTypeLabel(id: string): string {
  return TASK_LABELS[id] ?? id;
}

export function rewardTypeLabel(id: string): string {
  return REWARD_LABELS[id] ?? id;
}

export const SHAPE_OPTIONS: readonly QuestTypeOption[] = [
  { id: "", label: "(chapter default)" },
  { id: "circle", label: "Circle" },
  { id: "square", label: "Square" },
  { id: "rsquare", label: "Rounded square" },
  { id: "diamond", label: "Diamond" },
  { id: "hexagon", label: "Hexagon" },
  { id: "pentagon", label: "Pentagon" },
  { id: "gear", label: "Gear" },
  { id: "none", label: "None" },
] as const;

export const DEP_REQUIREMENT_OPTIONS: readonly QuestTypeOption[] = [
  { id: "", label: "(default)" },
  { id: "all_completed", label: "All completed" },
  { id: "one_completed", label: "One completed" },
  { id: "all_started", label: "All started" },
  { id: "one_started", label: "One started" },
] as const;
