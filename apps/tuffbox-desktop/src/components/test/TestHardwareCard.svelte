<script lang="ts">
  import type { LoadSample } from "../../lib/testLoad";
  import {
    hardwareLine,
    peaksFromSamples,
    recommendRam,
    waitingJvmLine,
  } from "../../lib/testLoad";

  let {
    samples,
    xmxMb = 4096,
  }: {
    samples: LoadSample[];
    xmxMb?: number;
  } = $props();

  const last = $derived(samples.length ? samples[samples.length - 1] : null);
  const peaks = $derived(peaksFromSamples(samples));
  const hasJvm = $derived(samples.some((s) => s.procRssMb > 0));
  const advice = $derived(
    peaks
      ? recommendRam(peaks.peakProcMb, xmxMb, peaks.peakHostMb, peaks.lastHostTotalMb)
      : null,
  );
  const line = $derived(
    !last
      ? "Run a profile to record RAM while the pack boots."
      : !hasJvm
        ? waitingJvmLine(last.hostUsedMb, last.hostTotalMb)
        : advice && peaks
          ? hardwareLine(peaks.peakProcMb, advice, peaks.peakHostMb, peaks.lastHostTotalMb)
          : waitingJvmLine(last.hostUsedMb, last.hostTotalMb),
  );
</script>

<div class={["hw-card", advice?.machine ?? ""]}>
  <p>{line}</p>
</div>

<style>
  .hw-card {
    min-width: 0;
    overflow: hidden;
    padding: 10px 12px;
    background: var(--bg-tertiary);
    border: 1px solid var(--border-color);
    border-radius: var(--border-radius-md);
  }
  .hw-card p {
    margin: 0;
    font-size: 12px;
    line-height: 1.45;
    color: var(--text-secondary);
    overflow-wrap: anywhere;
  }
  .hw-card.comfortable {
    border-color: color-mix(in srgb, var(--accent-primary) 28%, transparent);
  }
  .hw-card.tight {
    border-color: color-mix(in srgb, var(--accent-warning) 45%, transparent);
  }
  .hw-card.overloaded {
    border-color: color-mix(in srgb, var(--accent-danger) 45%, transparent);
  }
</style>
