/** Shared quest domain types for `@tuffbox/quest-lib` (structural; apps may extend). */

export interface QuestData {
  id: string;
  title: string;
  subtitle?: string | null;
  description: string[];
  x: number;
  y: number;
  icon?: string | Record<string, unknown> | null;
  dependencies: string[];
  tasks?: unknown[];
  rewards?: unknown[];
  optional?: boolean;
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
  titleFromSnbt?: boolean;
  subtitleFromSnbt?: boolean;
  descriptionFromSnbt?: boolean;
}

export interface QuestChapter {
  id: string;
  title: string;
  icon?: string | Record<string, unknown> | null;
  quests: QuestData[];
  group?: string | null;
  orderIndex?: number | null;
  filename?: string | null;
  defaultQuestShape?: string | null;
  defaultHideDependencyLines?: boolean | null;
  extras?: Record<string, unknown>;
  sourceFile?: string | null;
  titleFromSnbt?: boolean;
}

export interface QuestValidationIssue {
  questId: string;
  message: string;
}
