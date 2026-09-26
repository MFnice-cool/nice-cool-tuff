/** Biome colors for MCA Selector–style world map (matches Rust FNV-1a hash_biome_name). */

/** Same algorithm as `hash_biome_name` in region_edit.rs */
export function hashBiomeName(name: string): number {
  let h = 2166136261 >>> 0;
  const bytes = new TextEncoder().encode(name);
  for (let i = 0; i < bytes.length; i++) {
    h ^= bytes[i];
    h = Math.imul(h, 16777619) >>> 0;
  }
  return (h & 0x7fffffff) >>> 0;
}

/** Approximate MCA Selector–like top-down biome colors (RGB). */
const NAMED: Record<string, [number, number, number]> = {
  "minecraft:ocean": [64, 96, 180],
  "minecraft:deep_ocean": [48, 72, 160],
  "minecraft:warm_ocean": [72, 140, 200],
  "minecraft:lukewarm_ocean": [68, 120, 190],
  "minecraft:cold_ocean": [56, 88, 160],
  "minecraft:frozen_ocean": [140, 170, 210],
  "minecraft:deep_frozen_ocean": [120, 150, 200],
  "minecraft:river": [58, 110, 180],
  "minecraft:frozen_river": [160, 190, 220],
  "minecraft:beach": [210, 200, 130],
  "minecraft:snowy_beach": [230, 230, 240],
  "minecraft:stony_shore": [120, 120, 110],
  "minecraft:plains": [120, 180, 70],
  "minecraft:sunflower_plains": [140, 190, 60],
  "minecraft:forest": [55, 130, 45],
  "minecraft:flower_forest": [70, 150, 55],
  "minecraft:birch_forest": [90, 160, 70],
  "minecraft:old_growth_birch_forest": [80, 150, 65],
  "minecraft:dark_forest": [35, 90, 30],
  "minecraft:taiga": [70, 120, 70],
  "minecraft:old_growth_pine_taiga": [55, 100, 55],
  "minecraft:old_growth_spruce_taiga": [50, 95, 50],
  "minecraft:snowy_taiga": [170, 190, 180],
  "minecraft:snowy_plains": [220, 230, 240],
  "minecraft:ice_spikes": [200, 220, 245],
  "minecraft:desert": [220, 200, 110],
  "minecraft:savanna": [170, 160, 70],
  "minecraft:savanna_plateau": [160, 150, 65],
  "minecraft:windswept_savanna": [155, 145, 60],
  "minecraft:badlands": [180, 90, 45],
  "minecraft:wooded_badlands": [150, 85, 40],
  "minecraft:eroded_badlands": [190, 100, 50],
  "minecraft:jungle": [30, 140, 25],
  "minecraft:sparse_jungle": [50, 150, 40],
  "minecraft:bamboo_jungle": [40, 155, 35],
  "minecraft:swamp": [70, 100, 50],
  "minecraft:mangrove_swamp": [55, 95, 55],
  "minecraft:mushroom_fields": [160, 100, 160],
  "minecraft:meadow": [110, 175, 80],
  "minecraft:cherry_grove": [220, 160, 190],
  "minecraft:grove": [150, 170, 160],
  "minecraft:snowy_slopes": [200, 210, 220],
  "minecraft:frozen_peaks": [230, 235, 245],
  "minecraft:jagged_peaks": [180, 185, 190],
  "minecraft:stony_peaks": [140, 140, 135],
  "minecraft:windswept_hills": [110, 130, 90],
  "minecraft:windswept_forest": [80, 120, 70],
  "minecraft:windswept_gravelly_hills": [120, 120, 115],
  "minecraft:lush_caves": [80, 140, 90],
  "minecraft:dripstone_caves": [110, 95, 75],
  "minecraft:deep_dark": [30, 35, 45],
  "minecraft:the_void": [10, 10, 12],
  "minecraft:nether_wastes": [140, 50, 40],
  "minecraft:soul_sand_valley": [70, 55, 45],
  "minecraft:crimson_forest": [150, 40, 50],
  "minecraft:warped_forest": [30, 120, 110],
  "minecraft:basalt_deltas": [70, 70, 75],
  "minecraft:the_end": [180, 160, 200],
  "minecraft:end_highlands": [190, 170, 210],
  "minecraft:end_midlands": [170, 150, 190],
  "minecraft:small_end_islands": [160, 145, 185],
  "minecraft:end_barrens": [150, 140, 170],
};

/** Legacy numeric biome IDs (pre-1.18 style) → RGB. */
const LEGACY_ID: Record<number, [number, number, number]> = {
  0: [64, 96, 180],
  1: [120, 180, 70],
  2: [220, 200, 110],
  3: [110, 130, 90],
  4: [55, 130, 45],
  5: [70, 120, 70],
  6: [70, 100, 50],
  7: [58, 110, 180],
  8: [140, 50, 40],
  9: [180, 160, 200],
  10: [140, 170, 210],
  11: [160, 190, 220],
  12: [220, 230, 240],
  14: [160, 100, 160],
  16: [210, 200, 130],
  21: [30, 140, 25],
  27: [90, 160, 70],
  29: [35, 90, 30],
  32: [55, 100, 55],
  35: [170, 160, 70],
  37: [180, 90, 45],
};

const byHash = new Map<number, [number, number, number]>();
for (const [name, rgb] of Object.entries(NAMED)) {
  byHash.set(hashBiomeName(name), rgb);
  const short = name.replace(/^minecraft:/, "");
  byHash.set(hashBiomeName(short), rgb);
}

function hslFallback(id: number): [number, number, number] {
  let h = Math.imul(id ^ 0x9e3779b9, 0x85ebca6b) >>> 0;
  const hue = h % 360;
  // hsl → rgb approx at s=55% l=42%
  const s = 0.55;
  const l = 0.42;
  const c = (1 - Math.abs(2 * l - 1)) * s;
  const x = c * (1 - Math.abs(((hue / 60) % 2) - 1));
  const m = l - c / 2;
  let r = 0,
    g = 0,
    b = 0;
  if (hue < 60) [r, g, b] = [c, x, 0];
  else if (hue < 120) [r, g, b] = [x, c, 0];
  else if (hue < 180) [r, g, b] = [0, c, x];
  else if (hue < 240) [r, g, b] = [0, x, c];
  else if (hue < 300) [r, g, b] = [x, 0, c];
  else [r, g, b] = [c, 0, x];
  return [Math.round((r + m) * 255), Math.round((g + m) * 255), Math.round((b + m) * 255)];
}

export function biomeRgb(id: number): [number, number, number] {
  if (id < 0) return [26, 28, 34];
  if (LEGACY_ID[id]) return LEGACY_ID[id];
  const named = byHash.get(id >>> 0);
  if (named) return named;
  return hslFallback(id);
}

export function biomeCss(id: number, lightnessMul = 1): string {
  let [r, g, b] = biomeRgb(id);
  if (lightnessMul !== 1) {
    r = Math.max(0, Math.min(255, Math.round(r * lightnessMul)));
    g = Math.max(0, Math.min(255, Math.round(g * lightnessMul)));
    b = Math.max(0, Math.min(255, Math.round(b * lightnessMul)));
  }
  return `rgb(${r},${g},${b})`;
}

/** Map surfaceY into a lightness multiplier for terrain-like shading. */
export function heightShade(y: number | null | undefined, minY: number, maxY: number): number {
  if (y == null || y === -9999 || !Number.isFinite(y)) return 1;
  const span = Math.max(1, maxY - minY);
  const t = Math.max(0, Math.min(1, (y - minY) / span));
  // Low = darker (ocean/valleys), high = brighter (peaks)
  return 0.55 + t * 0.65;
}
