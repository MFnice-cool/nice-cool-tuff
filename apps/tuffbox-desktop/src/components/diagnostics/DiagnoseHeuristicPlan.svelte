<script lang="ts">
  let {
    plan,
    selectedOption = $bindable(),
    applying,
    onApply,
  }: {
    plan: any;
    selectedOption: number | null;
    applying: boolean;
    onApply: () => void;
  } = $props();
</script>

{#if plan}
  <div class="plan-card">
    <h3>Heuristic Fix plan</h3>
    <p>{plan.summary}</p>
    {#if (plan?.options?.length ?? 0) > 1}
      <div class="plan-options">
        <div class="plan-options-title">Which side to fix?</div>
        {#each plan.options as opt, i (i)}
          <label class="plan-option" class:preferred={opt?.preferred}>
            <input type="radio" name="fix-option" value={i} bind:group={selectedOption} />
            <span class="plan-option-label">
              {opt?.label ?? `Option ${i + 1}`}
              {#if opt?.preferred}<small class="muted-inline">recommended</small>{/if}
            </span>
            {#if opt?.reason}<small class="plan-option-reason">{opt.reason}</small>{/if}
          </label>
        {/each}
      </div>
    {/if}
    <button class="primary" onclick={onApply} disabled={applying}>
      {applying ? "Applying…" : "Apply heuristic fix plan"}
    </button>
  </div>
{/if}
