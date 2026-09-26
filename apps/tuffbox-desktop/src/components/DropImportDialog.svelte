<script lang="ts">
  import { Dialog as BitsDialog } from "bits-ui";
  import { fly } from "svelte/transition";
  import { quintOut } from "svelte/easing";
  import { Package } from "@lucide/svelte";
  import { onMount } from "svelte";

  type DropInspect = {
    kind?: string;
    format?: string;
    name?: string;
    totalEntries?: number;
    counts?: {
      mods?: number;
      resourcepacks?: number;
      shaderpacks?: number;
      files?: number;
    };
    present?: string[];
    missing?: string[];
    warnings?: string[];
  };

  let {
    inspect,
    busy = false,
    onconfirm,
    oncancel,
  }: {
    inspect: DropInspect;
    busy?: boolean;
    onconfirm?: (name: string) => void;
    oncancel?: () => void;
  } = $props();

  // Intentional snapshot: the dialog seeds its text field from the
  // backend-detected name exactly once at mount; later prop changes must
  // not overwrite user input (same pattern as PromptDialog).
  let name = $state("");

  onMount(() => {
    name = inspect.name || "";
  });

  const FORMAT_LABEL: Record<string, string> = {
    modrinth: "Modrinth pack",
    curseforge: "CurseForge pack",
    prism: "Prism / MultiMC instance",
    packwiz: "Packwiz pack",
    content: "Modpack content",
    instance: "Minecraft instance",
    empty: "Empty archive",
    unknown: "Unknown contents",
  };
  const KIND_LABEL: Record<string, string> = {
    zip: ".zip archive",
    mrpack: ".mrpack archive",
    rar: ".rar archive",
    "7z": ".7z archive",
    dir: "folder",
  };

  const formatLabel = $derived(FORMAT_LABEL[inspect.format ?? ""] ?? "Import");
  const kindLabel = $derived(KIND_LABEL[inspect.kind ?? ""] ?? "");
  const isContent = $derived(inspect.format === "content");
  const missing = $derived(inspect.missing ?? []);

  function submit() {
    if (!busy && name.trim()) onconfirm?.(name.trim());
  }
</script>

<BitsDialog.Root
  open={true}
  onOpenChange={(open) => {
    if (!open && !busy) oncancel?.();
  }}
>
  <BitsDialog.Portal>
    <div transition:fly={{ y: 14, duration: 200, opacity: 0, easing: quintOut }}>
      <BitsDialog.Overlay class="drop-backdrop" />
      <BitsDialog.Content class="drop-dialog" data-testid="library-drop-dialog">
        <BitsDialog.Title class="drop-title">Create a build from this import?</BitsDialog.Title>
        <BitsDialog.Description class="drop-sub">
          <Package size={14} />
          {formatLabel}{kindLabel ? ` · ${kindLabel}` : ""}{inspect.totalEntries
            ? ` · ${inspect.totalEntries} entries`
            : ""}
        </BitsDialog.Description>

        <div class="drop-counts" data-testid="drop-dialog-counts">
          <div><strong>{inspect.counts?.mods ?? 0}</strong><span>mods</span></div>
          <div><strong>{inspect.counts?.resourcepacks ?? 0}</strong><span>resourcepacks</span></div>
          <div><strong>{inspect.counts?.shaderpacks ?? 0}</strong><span>shaderpacks</span></div>
          <div><strong>{inspect.counts?.files ?? 0}</strong><span>files</span></div>
        </div>

        {#if isContent}
          <p class="drop-note">
            Loose content import — these folders become the new build's
            mods / resourcepacks / shaderpacks.
          </p>
        {:else if missing.length > 0}
          <div class="drop-missing" data-testid="drop-dialog-missing">
            <strong>Missing folders: </strong>
            <span>{missing.join(", ")}</span>
            <em>The build is created without them — fine for partial packs.</em>
          </div>
        {/if}

        {#each inspect.warnings ?? [] as warning (warning)}
          <p class="drop-warning">{warning}</p>
        {/each}

        <input
          class="drop-input"
          type="text"
          bind:value={name}
          data-testid="drop-dialog-name"
          aria-label="Build name"
          onkeydown={(e) => e.key === "Enter" && submit()}
        />

        <div class="drop-actions">
          <BitsDialog.Close
            class="drop-ghost"
            onclick={() => oncancel?.()}
            data-testid="drop-dialog-cancel"
          >
            Cancel
          </BitsDialog.Close>
          <button
            class="drop-primary"
            disabled={busy || !name.trim()}
            onclick={submit}
            data-testid="drop-dialog-create"
          >
            {busy ? "Creating…" : "Create build"}
          </button>
        </div>
      </BitsDialog.Content>
    </div>
  </BitsDialog.Portal>
</BitsDialog.Root>

<style>
  :global(.drop-backdrop) {
    position: fixed;
    inset: 0;
    background: rgba(0, 0, 0, 0.55);
    backdrop-filter: blur(8px);
    z-index: 200;
  }
  :global(.drop-dialog) {
    position: fixed;
    top: 50%;
    left: 50%;
    transform: translate(-50%, -50%);
    background: var(--bg-secondary);
    border: 1px solid var(--border-color);
    border-radius: var(--border-radius-xl);
    padding: 22px;
    width: min(460px, 92vw);
    box-shadow: var(--shadow-lg);
    z-index: 201;
    display: grid;
    gap: 12px;
  }
  :global(.drop-dialog .drop-title) {
    font-size: 17px;
    font-weight: 700;
    color: var(--text-primary);
    margin: 0;
  }
  :global(.drop-dialog .drop-sub) {
    display: inline-flex;
    align-items: center;
    gap: 8px;
    color: var(--accent-primary);
    font-size: 12px;
    font-weight: 700;
    margin: 0;
  }
  .drop-counts {
    display: grid;
    grid-template-columns: repeat(4, minmax(0, 1fr));
    gap: 8px;
  }
  .drop-counts div {
    background: var(--bg-tertiary);
    border: 1px solid var(--border-color);
    border-radius: var(--border-radius-md);
    padding: 8px 10px;
    display: grid;
    gap: 8px;
  }
  .drop-counts strong {
    font-size: 15px;
    color: var(--text-primary);
  }
  .drop-counts span {
    font-size: 12px;
    color: var(--text-muted);
  }
  .drop-note,
  .drop-warning {
    margin: 0;
    font-size: 12px;
    color: var(--text-muted);
    line-height: 1.45;
  }
  .drop-warning {
    color: var(--accent-warning, #f59e0b);
  }
  .drop-missing {
    display: grid;
    gap: 8px;
    font-size: 12px;
    color: var(--text-secondary);
    background: var(--bg-tertiary);
    border: 1px solid var(--border-color);
    border-radius: var(--border-radius-md);
    padding: 10px 12px;
  }
  .drop-missing span {
    color: var(--accent-warning, #f59e0b);
    font-weight: 600;
  }
  .drop-missing em {
    color: var(--text-muted);
    font-style: normal;
  }
  .drop-input {
    width: 100%;
    padding: 9px 12px;
    font-size: 13px;
    color: var(--text-primary);
    background: var(--bg-tertiary);
    border: 1px solid var(--border-color);
    border-radius: var(--border-radius-md);
  }
  .drop-actions {
    display: flex;
    gap: 10px;
    justify-content: flex-end;
  }
  :global(.drop-dialog .drop-ghost) {
    padding: 8px 14px;
    font-size: 13px;
    font-weight: 600;
    color: var(--text-secondary);
    background: transparent;
    border: 1px solid var(--border-color);
    border-radius: var(--border-radius-md);
    cursor: pointer;
  }
  .drop-primary {
    padding: 8px 14px;
    font-size: 13px;
    font-weight: 700;
    color: var(--on-accent, #000);
    background: var(--accent-primary);
    border: none;
    border-radius: var(--border-radius-md);
    cursor: pointer;
  }
  .drop-primary:disabled,
  :global(.drop-dialog .drop-ghost:disabled) {
    opacity: 0.55;
    cursor: not-allowed;
  }
</style>
