/** Client-side FTB quest dependency cycle check (questId would depend on depId).
 *  `depId` may be a quest id or a task id (FTB allows both). */
export function wouldCreateQuestCycle(
  questId: string,
  depId: string,
  quests: { id: string; dependencies: string[]; tasks?: { id: string }[] }[],
): boolean {
  if (!depId || depId === questId) return true;
  const byId = new Map(quests.map((q) => [q.id, q]));
  const taskOwner = new Map<string, string>();
  for (const q of quests) {
    for (const t of q.tasks ?? []) {
      if (t.id) taskOwner.set(t.id, q.id);
    }
  }
  const resolve = (id: string) => (byId.has(id) ? id : taskOwner.get(id) ?? id);
  const stack = [resolve(depId)];
  const seen = new Set<string>();
  while (stack.length) {
    const id = stack.pop()!;
    if (id === questId) return true;
    if (seen.has(id)) continue;
    seen.add(id);
    const node = byId.get(id);
    if (node) for (const d of node.dependencies) stack.push(resolve(d));
  }
  return false;
}

/** ponytail: tiny self-check — `npx tsx src/components/quests/deps.selfcheck.ts` */
export function selfCheckWouldCreateQuestCycle(): void {
  const qs = [
    { id: "a", dependencies: ["b"] },
    { id: "b", dependencies: ["c"] },
    { id: "c", dependencies: [] },
  ];
  if (!wouldCreateQuestCycle("c", "a", qs)) throw new Error("expected c→a to cycle");
  if (wouldCreateQuestCycle("b", "a", qs) !== true) throw new Error("expected b→a cycle");
  if (wouldCreateQuestCycle("a", "c", qs)) throw new Error("a→c is DAG edge, not a cycle");
  if (wouldCreateQuestCycle("a", "x", qs)) throw new Error("unknown id should not invent a cycle");
}
