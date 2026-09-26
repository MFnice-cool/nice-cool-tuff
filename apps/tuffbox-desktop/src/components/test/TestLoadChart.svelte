<script lang="ts">
  import type { LoadSample } from "../../lib/testLoad";
  import { gb1 } from "../../lib/testLoad";

  const VW = 1000;
  const VH = 160;

  let {
    samples,
    xmxMb = 4096,
    potato = false,
  }: {
    samples: LoadSample[];
    xmxMb?: number;
    potato?: boolean;
  } = $props();

  type Pt = { x: number; y: number };

  function yOf(pct: number): number {
    const p = Math.max(0, Math.min(100, pct));
    return VH - (p / 100) * VH;
  }

  function hostPct(s: LoadSample): number {
    return s.hostTotalMb > 0 ? (s.hostUsedMb / s.hostTotalMb) * 100 : 0;
  }

  function rssPct(s: LoadSample): number {
    return s.hostTotalMb > 0 ? (s.procRssMb / s.hostTotalMb) * 100 : 0;
  }

  function coords(list: LoadSample[], getPct: (s: LoadSample) => number): Pt[] {
    if (list.length === 0) return [];
    if (list.length === 1) {
      const y = yOf(getPct(list[0]));
      return [
        { x: 0, y },
        { x: VW, y },
      ];
    }
    const t0 = list[0].tSec;
    const t1 = list[list.length - 1].tSec;
    const span = Math.max(t1 - t0, 0.001);
    return list.map((s) => ({
      x: ((s.tSec - t0) / span) * VW,
      y: yOf(getPct(s)),
    }));
  }

  function lineD(pts: Pt[]): string {
    if (pts.length === 0) return "";
    return pts
      .map((p, i) => `${i === 0 ? "M" : "L"}${p.x.toFixed(1)},${p.y.toFixed(1)}`)
      .join(" ");
  }

  function areaD(pts: Pt[]): string {
    if (pts.length === 0) return "";
    const last = pts[pts.length - 1];
    return `${lineD(pts)} L${last.x.toFixed(1)},${VH} L${pts[0].x.toFixed(1)},${VH} Z`;
  }

  const last = $derived(samples.length ? samples[samples.length - 1] : null);
  const hostPts = $derived(coords(samples, hostPct));
  const rssPts = $derived(coords(samples, rssPct));
  const cpuPts = $derived(coords(samples, (s) => s.hostCpuPct));
  const hostArea = $derived(areaD(hostPts));
  const rssLine = $derived(lineD(rssPts));
  const cpuLine = $derived(lineD(cpuPts));
  const xmxPct = $derived(
    last && last.hostTotalMb > 0 ? Math.min(100, (xmxMb / last.hostTotalMb) * 100) : 0,
  );
  const xmxY = $derived(yOf(xmxPct));
  const peakRssMb = $derived(samples.reduce((m, s) => Math.max(m, s.procRssMb), 0));
  const peakHostPct = $derived(samples.reduce((m, s) => Math.max(m, hostPct(s)), 0));
  const peakCpu = $derived(samples.reduce((m, s) => Math.max(m, s.hostCpuPct), 0));
  const ariaLabel = $derived(
    last
      ? `Host RAM peak ${peakHostPct.toFixed(0)} percent, JVM RSS peak ${gb1(peakRssMb)} GB, CPU peak ${peakCpu.toFixed(0)} percent`
      : "No load samples yet",
  );
</script>

<div class={["load-chart", { potato }]}>
  {#if samples.length === 0}
    <div class="empty">Run a profile to record RAM while the pack boots.</div>
  {:else}
    <svg
      class="plot"
      viewBox="0 0 {VW} {VH}"
      preserveAspectRatio="none"
      role="img"
      aria-label={ariaLabel}
    >
      <title>{ariaLabel}</title>
      <rect class="zone ok" x="0" y={yOf(70)} width={VW} height={VH - yOf(70)} />
      <rect class="zone tight" x="0" y={yOf(90)} width={VW} height={yOf(70) - yOf(90)} />
      <rect class="zone hot" x="0" y="0" width={VW} height={yOf(90)} />
      {#if hostArea}
        <path class="host-area" d={hostArea} />
      {/if}
      {#if cpuLine}
        <path class="cpu-line" d={cpuLine} />
      {/if}
      {#if rssLine}
        <path class="rss-line" d={rssLine} />
      {/if}
      {#if xmxPct > 0}
        <line class="xmx" x1="0" y1={xmxY} x2={VW} y2={xmxY} />
      {/if}
    </svg>
    <div class="legend">
      <span><i class="swatch host"></i> Host RAM {last ? hostPct(last).toFixed(0) : "—"}%</span>
      <span><i class="swatch rss"></i> JVM peak {gb1(peakRssMb)} GB</span>
      <span>
        Host {last ? `${gb1(last.hostUsedMb)}/${gb1(last.hostTotalMb)}` : "—"} GB
      </span>
      <span><i class="swatch cpu"></i> CPU {last ? last.hostCpuPct.toFixed(0) : "—"}%</span>
      {#if xmxPct > 0}
        <span><i class="swatch xmx"></i> Xmx {gb1(xmxMb)} GB</span>
      {/if}
    </div>
  {/if}
</div>

<style>
  .load-chart {
    display: flex;
    flex-direction: column;
    min-width: 0;
    min-height: 0;
    gap: 8px;
    flex: 1;
  }
  .empty {
    flex: 1;
    min-height: 104px;
    display: flex;
    align-items: center;
    justify-content: center;
    text-align: center;
    padding: 16px;
    color: var(--text-muted);
    font-size: 13px;
    line-height: 1.45;
  }
  .plot {
    width: 100%;
    height: 160px;
    display: block;
    min-width: 0;
  }
  .zone.ok {
    fill: color-mix(in srgb, var(--accent-primary) 10%, transparent);
  }
  .zone.tight {
    fill: color-mix(in srgb, var(--accent-warning) 12%, transparent);
  }
  .zone.hot {
    fill: color-mix(in srgb, var(--accent-danger) 14%, transparent);
  }
  .host-area {
    fill: color-mix(in srgb, var(--accent-primary) 28%, transparent);
  }
  .rss-line {
    fill: none;
    stroke: var(--accent-secondary);
    stroke-width: 2;
    vector-effect: non-scaling-stroke;
  }
  .cpu-line {
    fill: none;
    stroke: var(--text-muted);
    stroke-width: 1;
    opacity: 0.45;
    vector-effect: non-scaling-stroke;
  }
  .xmx {
    stroke: var(--text-secondary);
    stroke-width: 1;
    stroke-dasharray: 6 5;
    vector-effect: non-scaling-stroke;
  }
  .legend {
    display: flex;
    flex-wrap: wrap;
    gap: 8px 14px;
    font-size: 12px;
    color: var(--text-muted);
    min-width: 0;
  }
  .legend span {
    display: inline-flex;
    align-items: center;
    gap: 8px;
    min-width: 0;
  }
  .swatch {
    width: 8px;
    height: 8px;
    display: inline-block;
    flex-shrink: 0;
    background: var(--text-muted);
  }
  .swatch.host {
    background: var(--accent-primary);
  }
  .swatch.rss {
    background: var(--accent-secondary);
  }
  .swatch.cpu {
    opacity: 0.45;
  }
  .swatch.xmx {
    background: transparent;
    border: 1px dashed var(--text-secondary);
    height: 0;
    width: 10px;
  }
  .potato .rss-line,
  .potato .cpu-line,
  .potato .host-area {
    transition: none;
  }
</style>
