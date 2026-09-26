<script lang="ts">
  import { ArrowUp } from "@lucide/svelte";
  import { fly } from "svelte/transition";

  let { container = null }: { container?: HTMLElement | null } = $props();

  let visible = $state(false);

  function check() {
    if (!container) return;
    visible = container.scrollTop > 200;
  }

  function scrollToTop() {
    container?.scrollTo({ top: 0, behavior: "smooth" });
  }

  $effect(() => {
    if (!container) return;
    container.addEventListener("scroll", check, { passive: true });
    check();
    return () => {
      container.removeEventListener("scroll", check);
    };
  });
</script>

{#if visible}
  <button
    class="scroll-top"
    onclick={scrollToTop}
    aria-label="Scroll to top"
    in:fly={{ y: 10, duration: 150 }}
    out:fly={{ y: 10, duration: 100 }}
  >
    <ArrowUp size={18} />
  </button>
{/if}

<style>
  .scroll-top {
    position: absolute;
    bottom: 20px;
    right: 20px;
    width: 40px;
    height: 40px;
    border-radius: 50%;
    background: var(--bg-elevated);
    border: 1px solid var(--border-color);
    color: var(--text-secondary);
    display: flex;
    align-items: center;
    justify-content: center;
    box-shadow: var(--shadow-md);
    z-index: 50;
    cursor: pointer;
    transition: all 0.15s ease;
  }

  .scroll-top:hover {
    background: var(--accent-primary);
    color: #000;
    border-color: var(--accent-primary);
    transform: translateY(-2px);
    box-shadow: var(--shadow-lg);
  }
</style>
