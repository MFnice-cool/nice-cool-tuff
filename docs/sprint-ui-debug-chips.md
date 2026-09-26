# Sprint: UI debug — inline chips & text overflow

Date: 2026-09-11. Branch: `arena/01a08f9f-tuffbox-build-test`.

## Symptom (user report, no screenshot)

Some rounded-corner blocks slide off their text; text escapes its bounds.
User pointed at "other" screens (not Home / Library / Settings).

## Root cause (static audit)

A whole class of the same defect: status chips/pills/badges/counts/`kbd`
are plain **inline** `<span>`/`<kbd>` with `background + padding +
border-radius`, but without `display`, `line-height`, `white-space` or
`vertical-align`. Consequences:

- vertical padding on inline elements doesn't take part in layout — the
  rounded block visually detaches from the text line ("съезжает с текста"),
  and can overlap neighbouring lines;
- `max-width` / `text-overflow: ellipsis` are silently ignored on inline
  elements — several rules already had the ellipsis trio that could never
  trigger, so long text escaped its bounds;
- `World .meta-pill` keeps `inline-flex` (it holds an icon) — ellipsis can't
  work there either, but `overflow: hidden` at least clips; added
  `min-width: 0` so it shrinks inside flex headers.

## Fix (CSS only, 22 files)

Text chips got `display: inline-block; line-height: 1.4;
white-space: nowrap; vertical-align: baseline; max-width: 100%;
overflow: hidden; text-overflow: ellipsis;` (trimmed where the rule already
had some of it — verified zero duplicate declarations afterwards):

- Snapshots: `.timeline-count`, `.op-badge` (ellipsis now actually works),
  `.actor-pill`, `.tag`
- Mods: `.tab-count`, `.dup-list/.conflicts-jars .pill`, `.installed-pill`,
  `code` (long jar names: `overflow-wrap: anywhere` +
  `box-decoration-break: clone` so wrapped backgrounds stay glued),
  new `.dup-list li > code { min-width: 0 }`
- Graph: `.conflict-badge`, `.dep-installed-pill`
- CatalogProjectView: `.chip`; ConfigEditor: `.lang-badge` (+ `line-height`
  for button `.chip`); ListingCardPreview: `.chip/.mr-badge/.cf-cats span`
- Diagnostics: `.health-chip`; Chats: checklist `.prio`; TestRuns: `.chip-pid`
- AccountManager: `.active-badge`; HomeInstanceShelf: `.shelf-count`
- DiagnoseTriagePanels: `.pill`; DiagnoseProblemsList: `.applied-chip`
- Quests: KubeJS `.pill` (ellipsis fixed) + `.badge`, node `.ch-badge`
  (ellipsis fixed, compact `line-height` kept), review `.mode-badge`,
  ItemStack `.badge`
- YoutubeQueueWindow: `.qw-count`, `.qw-now-badge`
- `kbd`: TestLabConsole (both rules), ShortcutsModal, Settings
  `.shortcut-row kbd` (`min-width: 60px` works now)
- World `.meta-pill`: `min-width: 0; line-height: 1.4` (flex kept for the icon)

Deliberately skipped: `Graph .chip` (no markup uses it — dead rule),
`CreationTrends .tag-chip` / `TuneAiSidebar .pill/.chip` /
`DiagnoseProblemsList .install-chip` (buttons — inline-block by default),
`KeyboardHelp kbd` (already centered flex with fixed height).

## Checks (sandbox)

- `svelte-check`: 0 errors / 138 warnings (baseline).
- `vitest`: 35/35. `lint:tokens`, `check:bridge`, `lint:lazy-views`: OK.
- CSS-only change; no Rust touched.

## Manual QA (needs eyes — sandbox can't render the app)

1. Mods → duplicates/versions rows: long jar filenames wrap inside the rounded
   `code` block, "newest"/"manifest" pills sit on the text line.
2. Snapshots timeline: op badges truncate with `…` at 140 px.
3. Graph conflicts panel: badges/pills aligned with their rows.
4. Catalog project page: category chips in one line, truncate in narrow pane.
5. World map header with copied chunks: clip pill doesn't push the header.
6. Quest nodes with chapter titles: badge truncates inside the node.
7. Any `kbd` (shortcuts modal, test console, settings): box hugs the key name.

## Follow-up

If the exact spot the user meant is not covered above, point at the screen
(or attach a screenshot) — the audit scripts from this sprint
(`box-decoration` / chip-rule / element-type greps) pinpoint the component
in minutes.
