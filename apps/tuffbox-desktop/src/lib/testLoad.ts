/** Keep in sync with `crates/tuffbox-core/src/test_load.rs`. */

export type LoadSample = {
  tSec: number;
  hostUsedMb: number;
  hostTotalMb: number;
  procRssMb: number;
  hostCpuPct: number;
};

export type MachineFit = "comfortable" | "tight" | "overloaded";

export type RamAdvice = {
  recommendedGb: number;
  machine: MachineFit;
  peakHostPct: number;
};

export type LoadPeaks = {
  peakProcMb: number;
  peakHostMb: number;
  lastHostTotalMb: number;
};

export const LOAD_SAMPLE_CAP = 360;

const RAM_STEPS_GB = [8, 12, 16, 24, 32] as const;
const OS_HEADROOM_MB = 4096;

export function pushLoadSample(
  buf: LoadSample[],
  sample: LoadSample,
  cap = LOAD_SAMPLE_CAP,
): LoadSample[] {
  buf.push(sample);
  if (buf.length > cap) buf.splice(0, buf.length - cap);
  return buf;
}

export function peaksFromSamples(buf: LoadSample[]): LoadPeaks | null {
  if (buf.length === 0) return null;
  let peakProcMb = 0;
  let peakHostMb = 0;
  let lastHostTotalMb = 0;
  for (const s of buf) {
    if (s.procRssMb > peakProcMb) peakProcMb = s.procRssMb;
    if (s.hostUsedMb > peakHostMb) peakHostMb = s.hostUsedMb;
    lastHostTotalMb = s.hostTotalMb;
  }
  return { peakProcMb, peakHostMb, lastHostTotalMb };
}

export function recommendRam(
  peakRssMb: number,
  xmxMb: number,
  peakHostMb: number,
  hostTotalMb: number,
): RamAdvice {
  const neededMb = Math.max(peakRssMb, xmxMb) + OS_HEADROOM_MB;
  const recommendedGb =
    RAM_STEPS_GB.find((gb) => gb * 1024 >= neededMb) ?? 32;

  const peakHostPct =
    hostTotalMb <= 0 ? 0 : (peakHostMb / hostTotalMb) * 100;

  let machine: MachineFit =
    peakHostPct >= 90 ? "overloaded" : peakHostPct >= 70 ? "tight" : "comfortable";

  if (hostTotalMb > 0 && recommendedGb * 1024 > hostTotalMb && machine === "comfortable") {
    machine = "tight";
  }

  return { recommendedGb, machine, peakHostPct };
}

export function gb1(mb: number): string {
  return (mb / 1024).toFixed(1);
}

export function hardwareLine(
  peakRssMb: number,
  advice: RamAdvice,
  peakHostMb: number,
  hostTotalMb: number,
): string {
  return `Peak JVM ${gb1(peakRssMb)} GB · this PC is ${advice.machine} (${gb1(peakHostMb)}/${gb1(hostTotalMb)} GB) · players need ${advice.recommendedGb} GB RAM`;
}

export function waitingJvmLine(hostUsedMb: number, hostTotalMb: number): string {
  return `Waiting for the JVM… host RAM ${gb1(hostUsedMb)}/${gb1(hostTotalMb)} GB`;
}
