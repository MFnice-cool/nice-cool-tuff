<script lang="ts">
  import {
    Bold,
    Italic,
    Strikethrough,
    Heading1,
    Heading2,
    Heading3,
    List,
    ListOrdered,
    ListTodo,
    Quote,
    Code,
    Table,
    Minus,
    Link2,
    Image as ImageIcon,
    FilePlus2,
  } from "@lucide/svelte";

  let {
    onBold = () => {},
    onItalic = () => {},
    onStrikethrough = () => {},
    onHeading1 = () => {},
    onHeading2 = () => {},
    onHeading3 = () => {},
    onBulletList = () => {},
    onNumberedList = () => {},
    onTaskList = () => {},
    onQuote = () => {},
    onInlineCode = () => {},
    onCodeBlock = () => {},
    onTable = () => {},
    onHorizontalRule = () => {},
    onLink = () => {},
    onImageUrl = () => {},
    onLocalImage = () => {},
  }: {
    onBold?: () => void;
    onItalic?: () => void;
    onStrikethrough?: () => void;
    onHeading1?: () => void;
    onHeading2?: () => void;
    onHeading3?: () => void;
    onBulletList?: () => void;
    onNumberedList?: () => void;
    onTaskList?: () => void;
    onQuote?: () => void;
    onInlineCode?: () => void;
    onCodeBlock?: () => void;
    onTable?: () => void;
    onHorizontalRule?: () => void;
    onLink?: () => void;
    onImageUrl?: () => void;
    onLocalImage?: () => void;
  } = $props();
</script>

<nav
  class="md-toolbar"
  aria-label="Markdown formatting"
>
  <!-- Text style -->
  <div class="toolbar-group">
    <button type="button" class="md-btn" title="Bold (**text**)" onclick={onBold} aria-label="Bold">
      <Bold size={14} />
    </button>
    <button type="button" class="md-btn" title="Italic (*text*)" onclick={onItalic} aria-label="Italic">
      <Italic size={14} />
    </button>
    <button type="button" class="md-btn" title="Strikethrough (~~text~~)" onclick={onStrikethrough} aria-label="Strikethrough">
      <Strikethrough size={14} />
    </button>
  </div>

  <span class="divider" aria-hidden="true"></span>

  <!-- Headings -->
  <div class="toolbar-group">
    <button type="button" class="md-btn" title="Heading 1 (# Heading)" onclick={onHeading1} aria-label="Heading 1">
      <Heading1 size={14} />
    </button>
    <button type="button" class="md-btn" title="Heading 2 (## Heading)" onclick={onHeading2} aria-label="Heading 2">
      <Heading2 size={14} />
    </button>
    <button type="button" class="md-btn" title="Heading 3 (### Heading)" onclick={onHeading3} aria-label="Heading 3">
      <Heading3 size={14} />
    </button>
  </div>

  <span class="divider" aria-hidden="true"></span>

  <!-- Lists & quotes -->
  <div class="toolbar-group">
    <button type="button" class="md-btn" title="Bullet list (- item)" onclick={onBulletList} aria-label="Bullet list">
      <List size={14} />
    </button>
    <button type="button" class="md-btn" title="Numbered list (1. item)" onclick={onNumberedList} aria-label="Numbered list">
      <ListOrdered size={14} />
    </button>
    <button type="button" class="md-btn" title="Task list (- [ ] task)" onclick={onTaskList} aria-label="Task list">
      <ListTodo size={14} />
    </button>
    <button type="button" class="md-btn" title="Quote (> text)" onclick={onQuote} aria-label="Quote">
      <Quote size={14} />
    </button>
  </div>

  <span class="divider" aria-hidden="true"></span>

  <!-- Code & structure -->
  <div class="toolbar-group">
    <button type="button" class="md-btn" title="Inline code (`code`)" onclick={onInlineCode} aria-label="Inline code">
      <Code size={14} />
    </button>
    <button type="button" class="md-btn font-code" title="Code block (```lang)" onclick={onCodeBlock} aria-label="Code block">
      <span>{"{ }"}</span>
    </button>
    <button type="button" class="md-btn" title="Table (| col1 | col2 |)" onclick={onTable} aria-label="Table">
      <Table size={14} />
    </button>
    <button type="button" class="md-btn" title="Horizontal divider (---)" onclick={onHorizontalRule} aria-label="Horizontal rule">
      <Minus size={14} />
    </button>
  </div>

  <span class="divider" aria-hidden="true"></span>

  <!-- Links & Media -->
  <div class="toolbar-group">
    <button type="button" class="md-btn" title="Link ([label](url))" onclick={onLink} aria-label="Link">
      <Link2 size={14} />
    </button>
    <button type="button" class="md-btn" title="Image URL (![alt](url))" onclick={onImageUrl} aria-label="Image URL">
      <ImageIcon size={14} />
    </button>
    <button type="button" class="md-btn" title="Insert local image file" onclick={onLocalImage} aria-label="Insert local image">
      <FilePlus2 size={14} />
    </button>
  </div>
</nav>

<style>
  .md-toolbar {
    display: inline-flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 8px;
    padding: 6px 10px;
    border-radius: var(--border-radius-md);
    border: 1px solid var(--border-color);
    background: color-mix(in srgb, var(--bg-secondary) 55%, transparent);
    backdrop-filter: blur(12px);
    box-shadow: inset 0 1px 0 color-mix(in srgb, var(--text-muted) 10%, transparent);
    user-select: none;
  }

  .toolbar-group {
    display: inline-flex;
    align-items: center;
    gap: 8px;
  }

  .divider {
    display: inline-block;
    width: 1px;
    height: 22px;
    background: var(--border-color);
    margin: 0 4px;
    flex-shrink: 0;
  }

  .md-btn {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 36px;
    height: 34px;
    border: 1px solid transparent;
    background: color-mix(in srgb, var(--bg-elevated) 64%, transparent);
    color: var(--text-primary);
    border-radius: var(--border-radius-sm);
    cursor: pointer;
    font-size: 12.5px;
    font-weight: 800;
    transition: background var(--motion-fast, 160ms) ease, color var(--motion-fast, 160ms) ease, transform var(--motion-fast, 160ms) ease;
  }
  .md-btn:hover {
    background: var(--bg-hover, rgba(255, 255, 255, 0.08));
    border-color: color-mix(in srgb, var(--accent-primary) 35%, transparent);
    color: var(--text-primary);
  }
  .md-btn :global(svg) {
    width: 17px;
    height: 17px;
    stroke-width: 2.35;
  }
  .md-btn:active {
    background: color-mix(in srgb, var(--accent-primary) 22%, transparent);
    color: var(--accent-primary);
  }

  .font-code {
    font-family: var(--font-mono, ui-monospace, monospace);
    font-size: 12px;
    letter-spacing: -0.35px;
  }
</style>
