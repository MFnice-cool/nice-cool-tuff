package dev.tuffbox.cosmetics;

import com.mojang.blaze3d.vertex.PoseStack;
import com.mojang.blaze3d.vertex.VertexConsumer;
import com.mojang.math.Axis;
import net.minecraft.client.model.PlayerModel;
import net.minecraft.client.player.AbstractClientPlayer;
import net.minecraft.client.renderer.MultiBufferSource;
import net.minecraft.client.renderer.entity.RenderLayerParent;
import net.minecraft.client.renderer.entity.layers.RenderLayer;
import net.minecraft.util.Mth;

/**
 * Head accessories: conical china hat + halo. Original geometry — not derived from third-party source.
 */
public final class ChinaHatFeatureRenderer<T extends AbstractClientPlayer, M extends PlayerModel<T>>
        extends RenderLayer<T, M> {

    private static final int SEGS = 36;
    private static final int FULLBRIGHT = 0xF000F0;

    public ChinaHatFeatureRenderer(RenderLayerParent<T, M> parent) {
        super(parent);
    }

    @Override
    public void render(
            PoseStack pose,
            MultiBufferSource buffers,
            int light,
            T player,
            float limbSwing,
            float limbSwingAmount,
            float partialTick,
            float ageInTicks,
            float netHeadYaw,
            float headPitch
    ) {
        String hat = CosmeticsProfiles.hatFor(player);
        if (hat == null || hat.isBlank()) return;

        pose.pushPose();
        getParentModel().getHead().translateAndRotate(pose);
        // sit just above the skull
        pose.translate(0.0, -0.42, 0.0);

        float t = ageInTicks + partialTick;
        VertexConsumer vc = buffers.getBuffer(CosmeticsRender.emissive());

        if (hat.equalsIgnoreCase("china")) {
            renderChina(pose, vc, t);
        } else if (hat.equalsIgnoreCase("halo")) {
            renderHalo(pose, vc, t);
        } else if (hat.equalsIgnoreCase("horns")) {
            renderHorns(pose, vc, t);
        } else if (hat.equalsIgnoreCase("crown")) {
            renderCrown(pose, vc, t);
        }

        pose.popPose();
    }

    private static void renderChina(PoseStack pose, VertexConsumer vc, float t) {
        float spin = t * 0.08f;
        pose.pushPose();
        pose.mulPose(Axis.YP.rotation(spin));

        float radius = 0.62f;
        float tipH = 0.32f;
        PoseStack.Pose last = pose.last();

        // rainbow cone
        for (int i = 0; i < SEGS; i++) {
            float a0 = (float) (i * Math.PI * 2.0 / SEGS);
            float a1 = (float) ((i + 1) * Math.PI * 2.0 / SEGS);
            float hue = (i / (float) SEGS + t * 0.02f) % 1f;
            int tip = CosmeticsRender.hsva(hue, 0.85f, 1f, 210);
            int rim = CosmeticsRender.hsva(hue + 0.05f, 0.7f, 1f, 140);
            float x0 = Mth.cos(a0) * radius;
            float z0 = Mth.sin(a0) * radius;
            float x1 = Mth.cos(a1) * radius;
            float z1 = Mth.sin(a1) * radius;
            CosmeticsRender.vert(vc, last, FULLBRIGHT, 0, tipH, 0, tip);
            CosmeticsRender.vert(vc, last, FULLBRIGHT, x0, 0, z0, rim);
            CosmeticsRender.vert(vc, last, FULLBRIGHT, x1, 0, z1, rim);
            // duplicate reverse for double-sided
            CosmeticsRender.vert(vc, last, FULLBRIGHT, 0, tipH, 0, tip);
            CosmeticsRender.vert(vc, last, FULLBRIGHT, x1, 0, z1, rim);
            CosmeticsRender.vert(vc, last, FULLBRIGHT, x0, 0, z0, rim);
        }

        // thin brim ring
        for (int i = 0; i < SEGS; i++) {
            float hue = (i / (float) SEGS + t * 0.02f) % 1f;
            int col = CosmeticsRender.hsva(hue, 0.9f, 1f, 200);
            float a0 = (float) (i * Math.PI * 2.0 / SEGS);
            float a1 = (float) ((i + 1) * Math.PI * 2.0 / SEGS);
            float ri = radius * 0.92f;
            float ro = radius * 1.08f;
            float c0 = Mth.cos(a0);
            float s0 = Mth.sin(a0);
            float c1 = Mth.cos(a1);
            float s1 = Mth.sin(a1);
            CosmeticsRender.vert(vc, last, FULLBRIGHT, c0 * ro, -0.01f, s0 * ro, col);
            CosmeticsRender.vert(vc, last, FULLBRIGHT, c1 * ro, -0.01f, s1 * ro, col);
            CosmeticsRender.vert(vc, last, FULLBRIGHT, c1 * ri, -0.01f, s1 * ri, col);
            CosmeticsRender.vert(vc, last, FULLBRIGHT, c0 * ri, -0.01f, s0 * ri, col);
        }

        // soft under-glow disc
        int glow = CosmeticsRender.hsva((t * 0.03f) % 1f, 0.5f, 1f, 50);
        CosmeticsRender.ring(pose, vc, FULLBRIGHT, -0.02f, 0f, radius * 0.55f, 16, glow);

        pose.popPose();
    }

    private static void renderHalo(PoseStack pose, VertexConsumer vc, float t) {
        pose.pushPose();
        pose.translate(0.0, 0.22, 0.0);
        pose.mulPose(Axis.XP.rotationDegrees(8f));
        pose.mulPose(Axis.YP.rotation(t * 0.12f));

        float r = 0.38f;
        float thick = 0.045f;
        for (int layer = 0; layer < 2; layer++) {
            float y = layer * 0.02f;
            float pulse = 0.92f + 0.08f * Mth.sin(t * 0.25f + layer);
            for (int i = 0; i < SEGS; i++) {
                float hue = (i / (float) SEGS + t * 0.015f) % 1f;
                int col = CosmeticsRender.hsva(hue, 0.35f, 1f, layer == 0 ? 220 : 90);
                float a0 = (float) (i * Math.PI * 2.0 / SEGS);
                float a1 = (float) ((i + 1) * Math.PI * 2.0 / SEGS);
                float ri = (r - thick) * pulse;
                float ro = r * pulse;
                float c0 = Mth.cos(a0);
                float s0 = Mth.sin(a0);
                float c1 = Mth.cos(a1);
                float s1 = Mth.sin(a1);
                PoseStack.Pose last = pose.last();
                CosmeticsRender.vert(vc, last, FULLBRIGHT, c0 * ro, y, s0 * ro, col);
                CosmeticsRender.vert(vc, last, FULLBRIGHT, c1 * ro, y, s1 * ro, col);
                CosmeticsRender.vert(vc, last, FULLBRIGHT, c1 * ri, y, s1 * ri, col);
                CosmeticsRender.vert(vc, last, FULLBRIGHT, c0 * ri, y, s0 * ri, col);
            }
        }
        pose.popPose();
    }

    /** Demon-style curved horns (procedural; Cosmetica-inspired accessory slot, original mesh). */
    private static void renderHorns(PoseStack pose, VertexConsumer vc, float t) {
        renderHorn(pose, vc, t, true);
        renderHorn(pose, vc, t, false);
    }

    private static void renderHorn(PoseStack pose, VertexConsumer vc, float t, boolean left) {
        pose.pushPose();
        float side = left ? -1f : 1f;
        pose.translate(side * 0.18, 0.12, -0.05);
        pose.mulPose(Axis.ZP.rotationDegrees(side * (-35f + Mth.sin(t * 0.15f) * 4f)));
        pose.mulPose(Axis.XP.rotationDegrees(-25f));
        PoseStack.Pose last = pose.last();
        int segs = 8;
        for (int i = 0; i < segs; i++) {
            float u0 = i / (float) segs;
            float u1 = (i + 1) / (float) segs;
            float y0 = u0 * 0.42f;
            float y1 = u1 * 0.42f;
            float r0 = 0.07f * (1f - u0 * 0.85f);
            float r1 = 0.07f * (1f - u1 * 0.85f);
            float bend = u0 * u0 * side * 0.12f;
            float bend1 = u1 * u1 * side * 0.12f;
            int col = CosmeticsRender.hsva(0.02f + u0 * 0.05f, 0.85f, 0.9f, 220);
            for (int j = 0; j < 6; j++) {
                float a0 = (float) (j * Math.PI * 2 / 6);
                float a1 = (float) ((j + 1) * Math.PI * 2 / 6);
                CosmeticsRender.vert(vc, last, FULLBRIGHT, Mth.cos(a0) * r0 + bend, y0, Mth.sin(a0) * r0, col);
                CosmeticsRender.vert(vc, last, FULLBRIGHT, Mth.cos(a1) * r0 + bend, y0, Mth.sin(a1) * r0, col);
                CosmeticsRender.vert(vc, last, FULLBRIGHT, Mth.cos(a1) * r1 + bend1, y1, Mth.sin(a1) * r1, col);
                CosmeticsRender.vert(vc, last, FULLBRIGHT, Mth.cos(a0) * r1 + bend1, y1, Mth.sin(a0) * r1, col);
            }
        }
        pose.popPose();
    }

    private static void renderCrown(PoseStack pose, VertexConsumer vc, float t) {
        pose.pushPose();
        pose.translate(0.0, 0.08, 0.0);
        pose.mulPose(Axis.YP.rotation(t * 0.04f));
        float bandR = 0.32f;
        int spikes = 7;
        PoseStack.Pose last = pose.last();
        // band
        for (int i = 0; i < SEGS; i++) {
            float hue = 0.12f + (i / (float) SEGS) * 0.05f;
            int col = CosmeticsRender.hsva(hue, 0.7f, 1f, 210);
            float a0 = (float) (i * Math.PI * 2 / SEGS);
            float a1 = (float) ((i + 1) * Math.PI * 2 / SEGS);
            float ri = bandR * 0.88f;
            float ro = bandR;
            float c0 = Mth.cos(a0), s0 = Mth.sin(a0);
            float c1 = Mth.cos(a1), s1 = Mth.sin(a1);
            CosmeticsRender.vert(vc, last, FULLBRIGHT, c0 * ro, 0, s0 * ro, col);
            CosmeticsRender.vert(vc, last, FULLBRIGHT, c1 * ro, 0, s1 * ro, col);
            CosmeticsRender.vert(vc, last, FULLBRIGHT, c1 * ri, 0.06f, s1 * ri, col);
            CosmeticsRender.vert(vc, last, FULLBRIGHT, c0 * ri, 0.06f, s0 * ri, col);
        }
        // spikes
        for (int i = 0; i < spikes; i++) {
            float a = (float) (i * Math.PI * 2 / spikes);
            float cx = Mth.cos(a) * bandR * 0.95f;
            float cz = Mth.sin(a) * bandR * 0.95f;
            float h = 0.18f + (i % 2 == 0 ? 0.08f : 0f);
            int col = CosmeticsRender.hsva(0.13f + i * 0.02f, 0.75f, 1f, 230);
            CosmeticsRender.vert(vc, last, FULLBRIGHT, cx, 0.06f, cz, col);
            CosmeticsRender.vert(vc, last, FULLBRIGHT, cx + Mth.cos(a + 0.2f) * 0.06f, 0.06f, cz + Mth.sin(a + 0.2f) * 0.06f, col);
            CosmeticsRender.vert(vc, last, FULLBRIGHT, cx, 0.06f + h, cz, col);
            CosmeticsRender.vert(vc, last, FULLBRIGHT, cx, 0.06f + h, cz, col);
            CosmeticsRender.vert(vc, last, FULLBRIGHT, cx + Mth.cos(a - 0.2f) * 0.06f, 0.06f, cz + Mth.sin(a - 0.2f) * 0.06f, col);
            CosmeticsRender.vert(vc, last, FULLBRIGHT, cx, 0.06f, cz, col);
        }
        pose.popPose();
    }
}
