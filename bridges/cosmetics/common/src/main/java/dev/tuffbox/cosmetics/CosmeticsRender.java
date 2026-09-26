package dev.tuffbox.cosmetics;

import com.mojang.blaze3d.vertex.PoseStack;
import com.mojang.blaze3d.vertex.VertexConsumer;
import dev.tuffbox.cosmetics.core.CosmeticsCore;
import net.minecraft.client.renderer.RenderType;
import net.minecraft.client.renderer.texture.OverlayTexture;
import net.minecraft.resources.ResourceLocation;
import net.minecraft.util.Mth;

/** Shared vertex / color helpers for cosmetics FX. Original code. */
public final class CosmeticsRender {
    /** Solid translucent fill (1.21 has no misc/white.png). */
    public static final ResourceLocation FILL =
            ResourceLocation.withDefaultNamespace("textures/block/white_wool.png");

    private CosmeticsRender() {}

    public static RenderType translucent() {
        return RenderType.entityTranslucent(FILL);
    }

    public static RenderType emissive() {
        return RenderType.entityTranslucentEmissive(FILL);
    }

    public static int hsva(float hue, float sat, float val, int alpha) {
        return CosmeticsCore.hsva(hue, sat, val, alpha);
    }

    public static void vert(
            VertexConsumer vc, PoseStack.Pose pose, int light,
            float x, float y, float z, int argb
    ) {
        vc.addVertex(pose, x, y, z)
                .setColor((argb >> 16) & 255, (argb >> 8) & 255, argb & 255, (argb >>> 24) & 255)
                .setUv(0.5f, 0.5f)
                .setOverlay(OverlayTexture.NO_OVERLAY)
                .setLight(light)
                .setNormal(pose, 0f, 1f, 0f);
    }

    public static void vertUv(
            VertexConsumer vc, PoseStack.Pose pose, int light,
            float x, float y, float z, float u, float v, int argb
    ) {
        vc.addVertex(pose, x, y, z)
                .setColor((argb >> 16) & 255, (argb >> 8) & 255, argb & 255, (argb >>> 24) & 255)
                .setUv(u, v)
                .setOverlay(OverlayTexture.NO_OVERLAY)
                .setLight(light)
                .setNormal(pose, 0f, 0f, 1f);
    }

    public static void ring(
            PoseStack pose, VertexConsumer vc, int light,
            float y, float rInner, float rOuter, int segs, int argb
    ) {
        PoseStack.Pose last = pose.last();
        for (int i = 0; i < segs; i++) {
            float a0 = (float) (i * Math.PI * 2 / segs);
            float a1 = (float) ((i + 1) * Math.PI * 2 / segs);
            float c0 = Mth.cos(a0);
            float s0 = Mth.sin(a0);
            float c1 = Mth.cos(a1);
            float s1 = Mth.sin(a1);
            vert(vc, last, light, c0 * rOuter, y, s0 * rOuter, argb);
            vert(vc, last, light, c1 * rOuter, y, s1 * rOuter, argb);
            vert(vc, last, light, c1 * rInner, y, s1 * rInner, argb);
            vert(vc, last, light, c0 * rInner, y, s0 * rInner, argb);
        }
    }
}
