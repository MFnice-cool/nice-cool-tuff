/**
 * Svelte action: focus an element when it is mounted.
 * Replaces the `autofocus` attribute (which trips the a11y lint) while
 * keeping the same UX for modal forms.
 */
export function focusInit(node: HTMLElement): { destroy: () => void } {
  // Wait a frame so the element is actually in the layout (modal transitions).
  const id = window.setTimeout(() => {
    try {
      node.focus({ preventScroll: true });
    } catch {
      node.focus();
    }
  }, 0);
  return {
    destroy() {
      window.clearTimeout(id);
    },
  };
}
