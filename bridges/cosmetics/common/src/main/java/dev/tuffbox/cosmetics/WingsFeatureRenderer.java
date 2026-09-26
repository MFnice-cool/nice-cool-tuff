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
 * Procedural wing membranes (angel / demon / fairy). No external textures required.
 */
public final class WingsFeatureRenderer<T extends AbstractClientPlayer, M extends PlayerModel<T>>
        extends RenderLayer<T, M> {

    private static final int FULLBRIGHT = 0xF000F0;

    public WingsFeatureRenderer(RenderLayerParent<T, M> parent) {
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
        String wings = CosmeticsProfiles.wingsFor(player);
        if (wings == null || wings.isBlank()) return;

        float t = ageInTicks + partialTick;
        float flap = Mth.sin(t * 0.45f) * 0.55f;
        float breathe = Mth.sin(t * 0.12f) * 0.04f;

        pose.pushPose();
        getParentModel().body.translateAndRotate(pose);
        pose.translate(0.0, 0.2 + breathe, 0.14);

        VertexConsumer vc = buffers.getBuffer(CosmeticsRender.emissive());
        renderWing(pose, vc, true, flap, t, wings);
        renderWing(pose, vc, false, flap, t, wings);
        pose.popPose();
    }

    private static void renderWing(
            PoseStack pose, VertexConsumer vc, boolean left, float flap, float t, String style
    ) {
        pose.pushPose();
        float side = left ? -1f : 1f;
        float baseYaw = switch (style) {
            case "demon" -> 38f;
            case "fairy" -> 48f;
            default -> 32f;
        };
        pose.mulPose(Axis.YP.rotationDegrees(side * (baseYaw + flap * 28f)));
        pose.mulPose(Axis.ZP.rotationDegrees(side * (10f - flap * 8f)));
        pose.mulPose(Axis.XP.rotationDegrees(-6f + flap * 4f));

        float w = switch (style) {
            case "fairy" -> 0.42f;
            case "demon" -> 0.62f;
            default -> 0.58f;
        };
        float h = switch (style) {
            case "fairy" -> 0.55f;
            case "demon" -> 0.72f;
            default -> 0.78f;
        };

        // layered feathers / membrane panels
        int layers = style.equals("fairy") ? 4 : 5;
        for (int layer = 0; layer < layers; layer++) {
            float lt = layer / (float) (layers - 1);
            float inset = lt * 0.12f;
            float lift = lt * 0.08f;
            float hue = switch (style) {
                case "demon" -> 0.0f + lt * 0.06f + Mth.sin(t * 0.08f) * 0.02f;
                case "fairy" -> (0.72f + lt * 0.15f + t * 0.01f) % 1f;
                default -> 0.55f + lt * 0.08f; // soft blue-white
            };
            float sat = style.equals("angel") ? 0.25f : 0.75f;
            float val = style.equals("demon") ? 0.85f : 1f;
            int alpha = 160 - layer * 18;
            int col = CosmeticsRender.hsva(hue, sat, val, Math.max(40, alpha));

            float x0 = side * inset;
            float x1 = side * (w - inset * 0.5f);
            float y0 = lift;
            float y1 = h * (1f - lt * 0.15f);
            float z0 = -0.02f - lt * 0.04f;
            float z1 = -0.08f - lt * 0.06f;

            PoseStack.Pose last = pose.last();
            // front
            CosmeticsRender.vert(vc, last, FULLBRIGHT, x0, y0, z0, col);
            CosmeticsRender.vert(vc, last, FULLBRIGHT, x1, y0 + 0.05f, z1, col);
            CosmeticsRender.vert(vc, last, FULLBRIGHT, x1 * 0.9f, y1, z1 - 0.02f, col);
            CosmeticsRender.vert(vc, last, FULLBRIGHT, x0, y1 * 0.92f, z0 - 0.01f, col);
            // back (double-sided)
            CosmeticsRender.vert(vc, last, FULLBRIGHT, x0, y0, z0, col);
            CosmeticsRender.vert(vc, last, FULLBRIGHT, x0, y1 * 0.92f, z0 - 0.01f, col);
            CosmeticsRender.vert(vc, last, FULLBRIGHT, x1 * 0.9f, y1, z1 - 0.02f, col);
            CosmeticsRender.vert(vc, last, FULLBRIGHT, x1, y0 + 0.05f, z1, col);
        }

        // tip glow
        if (style.equals("fairy") || style.equals("angel")) {
            int tip = CosmeticsRender.hsva(style.equals("fairy") ? (t * 0.02f) % 1f : 0.58f, 0.4f, 1f, 90);
            float tipX = side * w * 0.85f;
            float tipY = h * 0.85f;
            float s = 0.06f;
            PoseStack.Pose last = pose.last();
            CosmeticsRender.vert(vc, last, FULLBRIGHT, tipX - s, tipY - s, -0.1f, tip);
            CosmeticsRender.vert(vc, last, FULLBRIGHT, tipX + s, tipY - s, -0.1f, tip);
            CosmeticsRender.vert(vc, last, FULLBRIGHT, tipX + s, tipY + s, -0.1f, tip);
            CosmeticsRender.vert(vc, last, FULLBRIGHT, tipX - s, tipY + s, -0.1f, tip);
        }

        pose.popPose();
    }
}
