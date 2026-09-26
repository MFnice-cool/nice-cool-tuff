# Mod Conflict Category-Aware Resolution Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add category-aware conflict pair parsing, policy-based resolution ranking, and multi-option fix plan with persistence.

**Architecture:** Pipeline → `ConflictPair` parser → `PolicyEngine` ranks candidates by category/replaceability/dependents → `ChangePlan.options` (multi-choice) → UI review + persisting user preference.

**Tech Stack:** Rust (tuffbox-core), Svelte 5 (Diagnostics UI), d3? (no — graph not needed).

**Spec:** See [matrix of category interactions](docs/superpowers/plans/2026-08-16-mod-conflict-resolution.md#matrix-interactions).

---

## Matrix Interactions

| # | Cat A | Cat B | First move | Priority keep |
|---|-------|-------|------------|--------------|
| 1 | Optimization | Content | Disable optimization | content > optimization |
| 2 | Optimization | Shader | Disable one side, equal tie-broken by popularity | equal |
| 3 | Content | Library/Api | Keep API, install/update api | strongly API |
| 4 | Content | Legacy | Disable legacy | newer / popular |
| 5 | Content | Content | Both disable, keep that with fewer dependents | by dependents |
| 6 | Library | Library outdated | Update (not disable) | always update |
| 7 | Bridge-mid | Optimization | Disable bridge (not engine) | keep engine |
| 8 | Management | Management dup | Disable older / fewer dep | keep newer |
| 9 | Optimization | Optimization dup | Disable one | equal |
| 10 | Loader-mismatch | Any | Move/disable wrong-loader | — |
| 11 | Content | Worldgen | No action, suppress nag | — |
| 12 | Duplicate id | — | Disable older | keep newest |

---

## Implementation Tasks

### Task 1: ModCategory enum + classifier (`mod_category.rs`)
**Files:** `crates/tuffbox-core/src/mod_category.rs` (new), `crates/tuffbox-core/src/lib.rs` (mod entry), test in `mod_category.rs`.

### Task 2: ConflictPair parser (`mod_conflict.rs`)
**Files:** `crates/tuffbox-core/src/mod_conflict.rs` (new), test fixtures.

### Task 3: Policy engine (`resolve.rs`)
**Files:** `crates/tuffbox-core/src/resolve.rs` (new), test scenarios from matrix.

### Task 4: Integrate in `crash.rs` build_hints + create_crash_fix_plan
**Files:** `crates/tuffbox-core/src/crash.rs`

### Task 5: `resolver.rs` MOD_CONFLICT — category-aware choice
**Files:** `crates/tuffbox-core/src/resolver.rs`

### Task 6: AI-policy veto + heuristic_plan_from_context fix
**Files:** `src-tauri/src/lib.rs`, `crates/tuffbox-core/src/action_plan.rs`

### Task 7: `ChangePlan.options` field
**Files:** `crates/tuffbox-core/src/change_plan.rs`

### Task 8: Frontend Diagnostics.svelte — multi-option plan + preference store
**Files:** `apps/tuffbox-desktop/src/components/Diagnostics.svelte`, `apps/tuffbox-desktop/src/lib/store.ts`

### Task 9: Verification + docs
- Run all tests, clippy, svelte-check, lint:tokens
- Write ADR / doc note in `docs/`
