/** Presentation model used by the Quest Editor surface. The API adapter maps
 * FTB Quests SNBT into these stable editor concepts. */
export interface Task {
  id: string;
  type: string;
  title?: string;
  itemId?: string;
  count?: number;
  data?: Record<string, unknown>;
}

export interface Reward {
  id: string;
  type: string;
  title?: string;
  itemId?: string;
  count?: number;
  data?: Record<string, unknown>;
}

export interface Quest {
  id: string;
  title: string;
  subtitle?: string;
  description?: string;
  icon?: string;
  shape?: "circle" | "square" | "hexagon";
  x: number;
  y: number;
  tasks: Task[];
  rewards: Reward[];
  dependencies: string[];
  snbt?: Record<string, unknown>;
}

export interface Chapter {
  id: string;
  title: string;
  icon?: string;
  group?: string;
  shape?: Quest["shape"];
  quests: Quest[];
  snbt?: Record<string, unknown>;
}
