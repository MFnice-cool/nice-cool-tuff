/** Pointer → flow/world coordinates for the quest canvas. */

export type Point = { x: number; y: number };

/**
 * Convert a mouse/pointer event into canvas world space.
 * `panX` / `panY` / `zoom` are the Svelte Flow viewport transform
 * (`translate(panX, panY) scale(zoom)` on `.xyflow__viewport`).
 *
 * Formula: (client - containerOrigin - pan) / zoom
 */
export function getWorldCoordinates(
  event: Pick<MouseEvent, "clientX" | "clientY">,
  containerElement: HTMLElement,
  panX: number,
  panY: number,
  zoom: number,
): Point {
  const rect = containerElement.getBoundingClientRect();
  const z = zoom || 1;
  return {
    x: (event.clientX - rect.left - panX) / z,
    y: (event.clientY - rect.top - panY) / z,
  };
}

/** Axis-aligned rect intersection (inclusive edges). */
export function rectsIntersect(
  a: { x1: number; y1: number; x2: number; y2: number },
  b: { x1: number; y1: number; x2: number; y2: number },
): boolean {
  const ax1 = Math.min(a.x1, a.x2);
  const ay1 = Math.min(a.y1, a.y2);
  const ax2 = Math.max(a.x1, a.x2);
  const ay2 = Math.max(a.y1, a.y2);
  const bx1 = Math.min(b.x1, b.x2);
  const by1 = Math.min(b.y1, b.y2);
  const bx2 = Math.max(b.x1, b.x2);
  const by2 = Math.max(b.y1, b.y2);
  return ax1 <= bx2 && ax2 >= bx1 && ay1 <= by2 && ay2 >= by1;
}
