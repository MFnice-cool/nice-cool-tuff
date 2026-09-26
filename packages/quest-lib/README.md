# `@tuffbox/quest-lib`

Shared TypeScript helpers for FTB Quests editors (desktop + `quest-editor-web`).

Includes: SNBT parse/serialize, MC formatting, selection/search/history, layout, color manager, task/reward labels.

Apps keep thin re-exports under `src/lib/*` so existing relative imports keep working. Prefer importing from `@tuffbox/quest-lib` in new code.

Desktop load/save of quest books remains in Rust (`tuffbox-core`); this package is the TS-side shared toolkit.
