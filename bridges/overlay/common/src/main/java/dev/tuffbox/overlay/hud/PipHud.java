package dev.tuffbox.overlay.hud;

import dev.tuffbox.overlay.OverlayConfig;
import dev.tuffbox.overlay.OverlayScreen;
import dev.tuffbox.overlay.OverlayState;
import dev.tuffbox.overlay.OverlayTheme;
import dev.tuffbox.overlay.media.MediaController;
import dev.tuffbox.overlay.media.TextureBlit;
import net.minecraft.client.DeltaTracker;
import net.minecraft.client.Minecraft;
import net.minecraft.client.gui.GuiGraphics;

/**
 * Picture-in-picture HUD widget: keeps the video visible in a screen corner
 * after the overlay GUI is closed. Display-only — transport is on keybinds
 * (F9/F10, PgUp/PgDn) or by reopening the overlay (F8).
 */
public final class PipHud {
    private PipHud() {}

    private static final int[] WIDTHS = {192, 288, 384};
    private static final int MARGIN = 12;

    public static void render(GuiGraphics graphics, DeltaTracker tracker) {
        Minecraft mc = Minecraft.getInstance();
        if (mc.options.hideGui) {
            return;
        }
        if (mc.screen instanceof OverlayScreen) {
            return; // full surface is shown inside the overlay itself
        }
        OverlayConfig cfg = OverlayConfig.get();
        if (!cfg.master || !cfg.pipEnabled) {
            return;
        }
        MediaController media = OverlayState.get().media;
        if (!media.isActive()) {
            return;
        }

        int w = WIDTHS[Math.max(0, Math.min(cfg.pipScale, WIDTHS.length - 1))];
        int h = w * 9 / 16;
        int sw = mc.getWindow().getGuiScaledWidth();
        int sh = mc.getWindow().getGuiScaledHeight();

        int px;
        int py;
        switch (cfg.pipCorner) {
            case 0: px = MARGIN; py = MARGIN; break;
            case 2: px = MARGIN; py = sh - h - MARGIN - 20; break;
            case 3: px = sw - w - MARGIN; py = sh - h - MARGIN - 20; break;
            default: px = sw - w - MARGIN; py = MARGIN; break;
        }

        // Frame
        graphics.fill(px - 2, py - 2, px + w + 2, py + h + 16, OverlayTheme.RAIL_BG);
        graphics.fill(px - 2, py - 2, px + w + 2, py - 1, OverlayTheme.ACCENT);

        int tex = media.texture();
        if (tex >= 0 && media.videoWidth() > 0) {
            TextureBlit.drawFit(graphics, tex, px, py, w, h, media.videoWidth(), media.videoHeight());
        } else {
            graphics.fill(px, py, px + w, py + h, 0xFF000000);
            String msg = media.isBuffering() ? "Buffering…" : "Audio only";
            graphics.drawString(mc.font, msg, px + (w - mc.font.width(msg)) / 2, py + h / 2 - 4,
                    OverlayTheme.TEXT_DIM, false);
        }

        // Status strip under the frame
        String state = media.isPlaying() ? "▶" : media.isPaused() ? "❚❚" : "…";
        String title = media.currentTitle();
        if (title.isEmpty()) {
            title = media.currentUrl();
        }
        String strip = state + " " + title;
        graphics.drawString(mc.font, clip(mc.font, strip, w + 4), px, py + h + 4,
                OverlayTheme.TEXT_DIM, false);
    }

    private static String clip(net.minecraft.client.gui.Font font, String s, int maxW) {
        if (s == null) {
            return "";
        }
        if (font.width(s) <= maxW) {
            return s;
        }
        while (s.length() > 1 && font.width(s + "…") > maxW) {
            s = s.substring(0, s.length() - 1);
        }
        return s + "…";
    }
}
