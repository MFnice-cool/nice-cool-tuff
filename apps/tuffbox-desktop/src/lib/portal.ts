/** Move a node under `document.body` for stacking above chrome / safe fixed coords. */
export function portal(node: HTMLElement, target: HTMLElement | string = "body") {
  const el =
    typeof target === "string"
      ? (document.querySelector(target) as HTMLElement | null)
      : target;
  if (!el) return;

  // Detach from Svelte's mount point so zoom/stacking contexts on .app-shell
  // cannot clip or offset position:fixed overlays.
  el.appendChild(node);

  return {
    destroy() {
      if (node.parentNode) node.parentNode.removeChild(node);
    },
  };
}
