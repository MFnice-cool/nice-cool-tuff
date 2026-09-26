<script lang="ts">
  import { convertFileSrc } from "@tauri-apps/api/core";
  import { api } from "../lib/api";
  import type { SkinModelVariant } from "../lib/skinLibrary";

  let {
    src = null,
    cachedPath = null,
    variant = "classic",
    width = 72,
    height = 144,
  }: {
    /** Remote skin URL (https / data). */
    src?: string | null;
    /** Local absolute PNG path. */
    cachedPath?: string | null;
    variant?: SkinModelVariant;
    width?: number;
    height?: number;
  } = $props();

  let canvas = $state<HTMLCanvasElement | undefined>(undefined);
  let failed = $state(false);

  const dataCache = new Map<string, string>();

  async function resolveImageUrl(): Promise<string | null> {
    if (cachedPath) return convertFileSrc(cachedPath);
    if (!src) return null;
    if (src.startsWith("data:") || src.startsWith("blob:")) return src;
    const hit = dataCache.get(src);
    if (hit) return hit;
    try {
      const data = await api.mcAuth.getSkinBase64(src);
      dataCache.set(src, data);
      return data;
    } catch {
      return src;
    }
  }

  function blit(
    ctx: CanvasRenderingContext2D,
    img: CanvasImageSource,
    sx: number,
    sy: number,
    sw: number,
    sh: number,
    dx: number,
    dy: number,
    dw: number,
    dh: number,
  ) {
    ctx.drawImage(img, sx, sy, sw, sh, dx, dy, dw, dh);
  }

  function paintFront(
    ctx: CanvasRenderingContext2D,
    img: HTMLImageElement,
    slim: boolean,
    scale: number,
  ) {
    const armW = slim ? 3 : 4;
    const ox = Math.floor((width / scale - (armW + 8 + armW)) / 2);
    const oy = 0;
    const modern = img.naturalHeight >= 64;

    // Legs
    blit(ctx, img, 4, 20, 4, 12, (ox + armW) * scale, (oy + 20) * scale, 4 * scale, 12 * scale);
    if (modern) {
      blit(ctx, img, 20, 52, 4, 12, (ox + armW + 4) * scale, (oy + 20) * scale, 4 * scale, 12 * scale);
    } else {
      // Old 64×32: mirror right leg for left
      blit(ctx, img, 4, 20, 4, 12, (ox + armW + 4) * scale, (oy + 20) * scale, 4 * scale, 12 * scale);
    }
    // Body
    blit(ctx, img, 20, 20, 8, 12, (ox + armW) * scale, (oy + 8) * scale, 8 * scale, 12 * scale);
    // Arms
    blit(ctx, img, 44, 20, armW, 12, ox * scale, (oy + 8) * scale, armW * scale, 12 * scale);
    if (modern) {
      blit(
        ctx,
        img,
        36,
        52,
        armW,
        12,
        (ox + armW + 8) * scale,
        (oy + 8) * scale,
        armW * scale,
        12 * scale,
      );
    } else {
      blit(
        ctx,
        img,
        44,
        20,
        armW,
        12,
        (ox + armW + 8) * scale,
        (oy + 8) * scale,
        armW * scale,
        12 * scale,
      );
    }
    // Head + hat
    blit(ctx, img, 8, 8, 8, 8, (ox + armW) * scale, oy * scale, 8 * scale, 8 * scale);
    blit(ctx, img, 40, 8, 8, 8, (ox + armW) * scale, oy * scale, 8 * scale, 8 * scale);
  }

  async function draw() {
    const el = canvas;
    if (!el) return;
    failed = false;
    const ctx = el.getContext("2d");
    if (!ctx) return;
    ctx.imageSmoothingEnabled = false;
    ctx.clearRect(0, 0, width, height);

    const url = await resolveImageUrl();
    if (!url) {
      failed = true;
      return;
    }

    await new Promise<void>((resolve) => {
      const img = new Image();
      img.decoding = "async";
      img.onload = () => {
        try {
          // Fit classic 8+12+12 = 32 skin-px tall into canvas height
          const scale = Math.max(1, Math.floor(height / 32));
          paintFront(ctx, img, variant === "slim", scale);
        } catch {
          failed = true;
        }
        resolve();
      };
      img.onerror = () => {
        failed = true;
        resolve();
      };
      img.src = url;
    });
  }

  $effect(() => {
    void src;
    void cachedPath;
    void variant;
    void width;
    void height;
    if (!canvas) return;
    void draw();
  });
</script>

<canvas
  bind:this={canvas}
  class="skin-front"
  class:failed
  {width}
  {height}
  aria-hidden="true"
></canvas>

<style>
  .skin-front {
    display: block;
    image-rendering: pixelated;
    image-rendering: crisp-edges;
  }

  .skin-front.failed {
    opacity: 0.35;
  }
</style>
