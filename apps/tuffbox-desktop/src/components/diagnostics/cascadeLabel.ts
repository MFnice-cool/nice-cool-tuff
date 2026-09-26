/** Human-readable Crash Diagnose cascade stage for sticky UI chips. */
export function formatCascadeLabel(
  stage: string | null | undefined,
  tried?: string[],
): { label: string; detail: string | null } {
  const raw = String(stage ?? "").trim();
  const s = raw.toLowerCase();

  let label: string | null = null;
  if (
    s === "l1_hit" ||
    s.includes("swarm_capsule") ||
    s.includes("local_kb") ||
    s.includes("crash_kb") ||
    s === "kb" ||
    (s.includes("local") && s.includes("kb"))
  ) {
    label = "L1 · Known fix";
  } else if (s === "l2_hit" || s.includes("swarm_volunteer")) {
    label = "L2 · Community Fog";
  } else if (s === "l3_hit" || s === "ai" || s.includes("remote") || s.startsWith("l3_")) {
    label = "L3 · AI";
  } else if (s === "heuristic" || s.includes("heuristic")) {
    label = "Local heuristic";
  } else if (raw) {
    const humanized = raw
      .replace(/[_-]+/g, " ")
      .replace(/\s+/g, " ")
      .trim()
      .replace(/\b\w/g, (c) => c.toUpperCase());
    label = humanized || null;
  }

  const miss = (tried ?? []).map(String).find((t) => t.toLowerCase().startsWith("l2_miss:"));
  let detail: string | null = null;
  if (miss) {
    const short = miss.replace(/^l2_miss:/i, "").trim().slice(0, 96);
    detail = short ? `L2 miss: ${short}` : null;
  }

  return { label: label ?? "", detail };
}
