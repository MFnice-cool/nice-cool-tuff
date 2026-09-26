/**
 * CodeMirror theme built ENTIRELY from the app's design tokens (CSS vars).
 *
 * The built-in editors (Tune config editor, KubeJS panel, listing brief)
 * used to hard-code `oneDark`, so switching the app to a light theme
 * (TuffBox Light, Win95) left a jarring dark slab in the middle of the UI.
 * Mapping every CM surface to `var(--…)` tokens makes the editor follow ALL
 * 18 themes — and any theme added later — with zero per-theme work.
 */
import { EditorView } from "@codemirror/view";
import { HighlightStyle, syntaxHighlighting } from "@codemirror/language";
import { tags as t } from "@lezer/highlight";

/** App themes with light canvases — the editor flips its `dark` flag for them. */
export const LIGHT_EDITOR_THEMES = new Set<string>(["tuffbox-light", "win95"]);

/** True when the editor should use the dark CodeMirror base for this app theme. */
export function isDarkEditorTheme(themeId: string | null | undefined): boolean {
  return !LIGHT_EDITOR_THEMES.has(String(themeId ?? "").toLowerCase());
}

/** Token-based syntax colors: works on every theme because the tokens are AA. */
const tokenSyntax = HighlightStyle.define([
  { tag: t.comment, color: "var(--text-muted)", fontStyle: "italic" },
  { tag: [t.keyword, t.operator, t.controlKeyword, t.moduleKeyword, t.definitionKeyword], color: "var(--accent-secondary)" },
  { tag: [t.string, t.special(t.string), t.character], color: "var(--accent-primary)" },
  { tag: [t.number, t.bool, t.atom], color: "var(--accent-warning)" },
  // JSON/TOML/properties keys and YAML keys read as emphasized text.
  { tag: [t.propertyName, t.tagName, t.labelName], color: "var(--text-primary)", fontWeight: "600" },
  { tag: [t.attributeName, t.attributeValue], color: "var(--accent-primary)" },
  { tag: [t.link, t.url], color: "var(--accent-primary)", textDecoration: "underline" },
  { tag: [t.meta, t.processingInstruction], color: "var(--text-muted)" },
  { tag: t.invalid, color: "var(--accent-danger)" },
]);

/**
 * Editor extensions for the current app theme.
 * `dark` only picks CM's internal base defaults; every visible color comes
 * from the theme tokens below.
 */
export function editorThemeFor(themeId: string | null | undefined) {
  const dark = isDarkEditorTheme(themeId);
  return [
    EditorView.theme(
      {
        "&": {
          color: "var(--text-primary)",
          backgroundColor: "var(--bg-secondary)",
        },
        ".cm-content": {
          caretColor: "var(--accent-primary)",
        },
        ".cm-cursor, .cm-dropCursor": {
          borderLeftColor: "var(--accent-primary)",
        },
        "&.cm-focused > .cm-scroller > .cm-selectionLayer .cm-selectionBackground, .cm-selectionBackground, .cm-content ::selection":
          {
            backgroundColor: "color-mix(in srgb, var(--accent-primary) 28%, transparent)",
          },
        ".cm-gutters": {
          backgroundColor: "var(--bg-primary)",
          color: "var(--text-muted)",
          borderRight: "1px solid var(--border-color)",
        },
        ".cm-activeLine": {
          backgroundColor: "color-mix(in srgb, var(--accent-primary) 7%, transparent)",
        },
        ".cm-activeLineGutter": {
          backgroundColor: "color-mix(in srgb, var(--accent-primary) 12%, transparent)",
          color: "var(--text-secondary)",
        },
        ".cm-selectionMatch": {
          backgroundColor: "color-mix(in srgb, var(--accent-secondary) 22%, transparent)",
        },
        ".cm-matchingBracket, &.cm-focused .cm-matchingBracket": {
          backgroundColor: "color-mix(in srgb, var(--accent-primary) 22%, transparent)",
          outline: "1px solid color-mix(in srgb, var(--accent-primary) 60%, transparent)",
          color: "inherit",
        },
        ".cm-tooltip": {
          backgroundColor: "var(--bg-elevated)",
          color: "var(--text-primary)",
          border: "1px solid var(--border-color)",
          borderRadius: "var(--border-radius-sm)",
          boxShadow: "var(--shadow-md)",
        },
        ".cm-tooltip.cm-tooltip-autocomplete > ul > li[aria-selected]": {
          backgroundColor: "color-mix(in srgb, var(--accent-primary) 18%, transparent)",
          color: "var(--text-primary)",
        },
        ".cm-completionIcon": {
          color: "var(--accent-secondary)",
        },
        ".cm-completionDetail": {
          color: "var(--text-muted)",
        },
        ".cm-panels": {
          backgroundColor: "var(--bg-tertiary)",
          color: "var(--text-primary)",
        },
        ".cm-searchMatch": {
          backgroundColor: "color-mix(in srgb, var(--accent-warning) 32%, transparent)",
        },
        ".cm-searchMatch-selected": {
          backgroundColor: "color-mix(in srgb, var(--accent-danger) 32%, transparent)",
        },
        ".cm-foldPlaceholder": {
          backgroundColor: "var(--bg-tertiary)",
          border: "1px solid var(--border-color)",
          color: "var(--text-muted)",
        },
      },
      { dark },
    ),
    syntaxHighlighting(tokenSyntax),
  ];
}
