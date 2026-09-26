import DOMPurify from "isomorphic-dompurify";

/** Sanitize HTML before `{@html ...}` — defense-in-depth alongside CSP. */
export function sanitizeHtml(dirty: string): string {
  return DOMPurify.sanitize(dirty, {
    USE_PROFILES: { html: true },
    FORBID_TAGS: ["script", "iframe", "object", "embed", "form", "link", "meta", "base"],
    FORBID_ATTR: ["style"],
  });
}
