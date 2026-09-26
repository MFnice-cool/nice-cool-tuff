package dev.tuffbox.overlay.media;

import com.mojang.blaze3d.pipeline.RenderTarget;
import com.mojang.blaze3d.systems.RenderSystem;
import com.mojang.blaze3d.vertex.BufferBuilder;
import com.mojang.blaze3d.vertex.BufferUploader;
import com.mojang.blaze3d.vertex.DefaultVertexFormat;
import com.mojang.blaze3d.vertex.Tesselator;
import com.mojang.blaze3d.vertex.VertexFormat;
import net.minecraft.client.gui.GuiGraphics;
import net.minecraft.client.renderer.GameRenderer;
import org.joml.Matrix4f;

/**
 * Draws a raw GL texture id (WATERMeDIA frame texture) into the GUI.
 * VLC frames arrive bottom-up, so V is flipped here.
 */
public final class TextureBlit {
    private TextureBlit() {}

    /** Draw texture fitted inside the box, letterboxed to keep aspect. */
    public static void drawFit(GuiGraphics graphics, int textureId,
                               int boxX, int boxY, int boxW, int boxH,
                               int videoW, int videoH) {
        if (textureId < 0 || boxW <= 0 || boxH <= 0) {
            return;
        }
        float boxAspect = (float) boxW / (float) boxH;
        float vidAspect = (videoW > 0 && videoH > 0) ? (float) videoW / (float) videoH : 16f / 9f;

        int w;
        int h;
        if (vidAspect > boxAspect) {
            w = boxW;
            h = Math.round(boxW / vidAspect);
        } else {
            h = boxH;
            w = Math.round(boxH * vidAspect);
        }
        int x = boxX + (boxW - w) / 2;
        int y = boxY + (boxH - h) / 2;
        draw(graphics, textureId, x, y, w, h);
    }

    public static void draw(GuiGraphics graphics, int textureId, int x, int y, int w, int h) {
        RenderSystem.enableBlend();
        RenderSystem.setShader(GameRenderer::getPositionTexShader);
        RenderSystem.setShaderTexture(0, textureId);
        Matrix4f pose = graphics.pose().last().pose();
        BufferBuilder buf = Tesselator.getInstance()
                .begin(VertexFormat.Mode.QUADS, DefaultVertexFormat.POSITION_TEX);
        buf.addVertex(pose, x, y + h, 0).setUv(0, 1);
        buf.addVertex(pose, x + w, y + h, 0).setUv(1, 1);
        buf.addVertex(pose, x + w, y, 0).setUv(1, 0);
        buf.addVertex(pose, x, y, 0).setUv(0, 0);
        BufferUploader.drawWithShader(buf.buildOrThrow());
        RenderSystem.disableBlend();
    }
}
