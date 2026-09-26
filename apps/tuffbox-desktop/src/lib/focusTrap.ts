/**
 * Svelte action that traps keyboard focus within the node.
 * Tab / Shift+Tab cycles through focusable elements inside.
 * Escape key fires the optional `onEscape` callback.
 * On destroy, restores focus to the element that was active before the trap mounted.
 *
 * Usage:
 *   <div use:trapFocus={{ onEscape: () => close() }}>
 */
export function trapFocus(
  node: HTMLElement,
  options?: { onEscape?: () => void; enabled?: boolean }
) {
  let opts = options ?? {};
  const selector = 'a[href], button:not([disabled]), input:not([disabled]), select:not([disabled]), textarea:not([disabled]), [tabindex]:not([tabindex="-1"])';

  const previouslyFocused =
    document.activeElement instanceof HTMLElement ? document.activeElement : null;

  function getFocusable(): HTMLElement[] {
    return Array.from(node.querySelectorAll<HTMLElement>(selector)).filter(
      (el) => !el.closest('[inert]') && el.offsetParent !== null
    );
  }

  function handleKey(e: KeyboardEvent) {
    if (opts.enabled === false) return;

    if (e.key === "Escape") {
      e.preventDefault();
      e.stopPropagation();
      opts.onEscape?.();
      return;
    }

    if (e.key !== "Tab") return;

    const focusable = getFocusable();
    if (focusable.length === 0) return;

    const first = focusable[0];
    const last = focusable[focusable.length - 1];

    if (e.shiftKey) {
      if (document.activeElement === first) {
        e.preventDefault();
        last.focus();
      }
    } else {
      if (document.activeElement === last) {
        e.preventDefault();
        first.focus();
      }
    }
  }

  // Focus the first focusable element on mount (modal only).
  if (opts.enabled !== false) {
    const focusable = getFocusable();
    if (focusable.length > 0) {
      requestAnimationFrame(() => {
        const autofocus = node.querySelector<HTMLElement>("[autofocus]");
        (autofocus || focusable[0]).focus();
      });
    }
  }

  node.addEventListener("keydown", handleKey);

  return {
    update(newOpts?: { onEscape?: () => void; enabled?: boolean }) {
      opts = newOpts ?? {};
    },
    destroy() {
      node.removeEventListener("keydown", handleKey);
      if (
        previouslyFocused &&
        document.contains(previouslyFocused) &&
        typeof previouslyFocused.focus === "function"
      ) {
        previouslyFocused.focus({ preventScroll: true });
      }
    },
  };
}
