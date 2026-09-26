/**
 * In-memory FTB Quests book validation for the desktop editor.
 * Mirrors quest-editor-web `validateQuestBook`, with book-wide dep/task resolution
 * (cross-chapter deps are allowed), plus optional catalog + reachability checks.
 */

import type { QuestChapter, QuestChapterGroup, QuestData, QuestValidationIssue } from "./api";
import { iconDisplayId } from "./api";
import { isItemObject, listFilterChildren, stackDisplayId, type ItemValue } from "./itemStack";

export function validateQuestBook(
  book: {
    chapters: QuestChapter[];
    chapterGroups?: QuestChapterGroup[];
  },
  options?: {
    availableItems?: Set<string> | null;
  },
): QuestValidationIssue[] {
  const issues: QuestValidationIssue[] = [];
  const allQuestIds = new Set<string>();
  const allTaskIds = new Set<string>();
  const allQuests: QuestData[] = [];

  for (const ch of book.chapters) {
    for (const q of ch.quests) {
      if (allQuestIds.has(q.id)) {
        issues.push({ questId: q.id, message: `Duplicate quest id "${q.id}"` });
      }
      allQuestIds.add(q.id);
      allQuests.push(q);
      for (const t of q.tasks ?? []) {
        if (t.id) allTaskIds.add(t.id);
      }
      if (!q.title?.trim()) {
        issues.push({ questId: q.id, message: "Empty title" });
      }
      if (!q.tasks?.length) {
        issues.push({ questId: q.id, message: "No tasks defined" });
      }
      for (const t of q.tasks ?? []) {
        if (t.type === "item" && !itemTaskHasItem(t.properties?.item)) {
          issues.push({ questId: q.id, message: "Item task has no item" });
        }
      }
      if (!iconDisplayId(q.icon)) {
        const hasItemVisual = (q.tasks ?? []).some(
          (t) => t.type === "item" && itemTaskHasItem(t.properties?.item),
        );
        if (!hasItemVisual) {
          issues.push({ questId: q.id, message: "Missing quest icon" });
        }
      }
    }
  }

  for (const q of allQuests) {
    for (const dep of q.dependencies ?? []) {
      if (!allQuestIds.has(dep) && !allTaskIds.has(dep)) {
        issues.push({ questId: q.id, message: `Missing dependency "${dep}"` });
      }
    }
  }

  // Book-wide cycle detection (quest → resolve task owner → quest)
  const byId = new Map(allQuests.map((q) => [q.id, q]));
  const taskOwner = new Map<string, string>();
  for (const q of allQuests) {
    for (const t of q.tasks ?? []) {
      if (t.id) taskOwner.set(t.id, q.id);
    }
  }
  const resolve = (id: string) => (byId.has(id) ? id : taskOwner.get(id) ?? id);

  const visited = new Set<string>();
  const inStack = new Set<string>();
  const cycleReported = new Set<string>();

  function hasCycle(qid: string): boolean {
    if (inStack.has(qid)) return true;
    if (visited.has(qid)) return false;
    visited.add(qid);
    inStack.add(qid);
    const q = byId.get(qid);
    if (q) {
      for (const dep of q.dependencies ?? []) {
        if (hasCycle(resolve(dep))) return true;
      }
    }
    inStack.delete(qid);
    return false;
  }

  for (const q of allQuests) {
    visited.clear();
    inStack.clear();
    if (hasCycle(q.id) && !cycleReported.has(q.id)) {
      issues.push({ questId: q.id, message: "Dependency cycle detected" });
      cycleReported.add(q.id);
    }
  }

  // Reachability from roots (quests with no dependencies)
  for (const qid of unreachableQuestIds(allQuests, taskOwner)) {
    issues.push({ questId: qid, message: "Unreachable from any root quest" });
  }

  const available = options?.availableItems ?? null;
  if (available && available.size > 0) {
    for (const q of allQuests) {
      for (const item of extractQuestItemIds(q)) {
        if (!item || item.startsWith("#") || item.startsWith("itemfilters:")) continue;
        if (!available.has(item)) {
          issues.push({ questId: q.id, message: `Unknown item '${item}'` });
        }
      }
    }
  }

  void book.chapterGroups;
  return issues;
}

/** BFS from dependency-free roots along dependent edges. */
export function unreachableQuestIds(
  allQuests: QuestData[],
  taskOwner: Map<string, string>,
): string[] {
  if (allQuests.length === 0) return [];
  const questIds = new Set(allQuests.map((q) => q.id));
  const resolve = (id: string) => (questIds.has(id) ? id : taskOwner.get(id) ?? id);

  const dependents = new Map<string, string[]>();
  for (const q of allQuests) {
    for (const dep of q.dependencies ?? []) {
      const r = resolve(dep);
      if (!questIds.has(r)) continue;
      const list = dependents.get(r) ?? [];
      list.push(q.id);
      dependents.set(r, list);
    }
  }

  const roots = allQuests.filter((q) => !(q.dependencies?.length)).map((q) => q.id);
  if (roots.length === 0) return [];

  const reachable = new Set<string>();
  const stack = [...roots];
  while (stack.length) {
    const id = stack.pop()!;
    if (reachable.has(id)) continue;
    reachable.add(id);
    for (const kid of dependents.get(id) ?? []) stack.push(kid);
  }

  return allQuests
    .filter((q) => {
      if (reachable.has(q.id)) return false;
      return (q.dependencies ?? []).every((d) => {
        const r = resolve(d);
        return questIds.has(r) || taskOwner.has(d);
      });
    })
    .map((q) => q.id);
}

function collectItemIds(v: unknown, out: string[]) {
  if (typeof v === "string") {
    if (v.trim()) out.push(v.trim());
    return;
  }
  if (!isItemObject(v)) return;
  const display = stackDisplayId(v as ItemValue);
  if (display) out.push(display);
  for (const child of listFilterChildren(v)) {
    collectItemIds(child, out);
  }
}

function itemTaskHasItem(v: unknown): boolean {
  if (typeof v === "string") return v.trim().length > 0;
  if (!v || typeof v !== "object") return false;
  if (isItemObject(v)) return !!stackDisplayId(v as ItemValue);
  return false;
}

/** Concrete item ids referenced by a quest (icon / item tasks / item rewards). */
export function extractQuestItemIds(q: QuestData): string[] {
  const out: string[] = [];
  const iconId = iconDisplayId(q.icon);
  if (iconId) out.push(iconId);
  for (const t of q.tasks ?? []) {
    if (t.type === "item") collectItemIds(t.properties?.item, out);
  }
  for (const r of q.rewards ?? []) {
    if (r.type === "item") collectItemIds(r.properties?.item, out);
  }
  return out;
}
