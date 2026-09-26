package dev.tuffbox.overlay.panel;

import dev.tuffbox.overlay.OverlayConfig;
import dev.tuffbox.overlay.OverlayState;
import dev.tuffbox.overlay.OverlayTheme;
import dev.tuffbox.overlay.feed.YoutubeFeed;
import dev.tuffbox.overlay.media.MediaController;
import dev.tuffbox.overlay.media.TextureBlit;
import dev.tuffbox.overlay.widgets.Ui;
import net.minecraft.client.Minecraft;
import net.minecraft.client.gui.Font;
import net.minecraft.client.gui.GuiGraphics;
import net.minecraft.client.gui.components.EditBox;
import net.minecraft.network.chat.Component;

import java.util.ArrayList;
import java.util.List;

/**
 * YouTube app page: URL/search input, video surface (WATERMeDIA), transport
 * controls, queue and a browsable feed pool. Playback survives closing the
 * overlay — audio keeps going, optionally with the PiP HUD widget.
 */
public final class YoutubePanel extends Panel {
    private static final int TRANSPORT_H = 20;
    private static final int SEEK_H = 14;

    private EditBox input;
    private final List<Ui.Button> transport = new ArrayList<Ui.Button>();
    private final List<Ui.Button> pipBar = new ArrayList<Ui.Button>();

    private List<YoutubeFeed.FeedVideo> pool = new ArrayList<YoutubeFeed.FeedVideo>();
    private List<YoutubeFeed.FeedVideo> filtered = new ArrayList<YoutubeFeed.FeedVideo>();
    private boolean poolLoading;
    private boolean poolFailed;
    private int scroll;

    private String statusHint = "";
    /** Survives init() on resize/page switches. */
    private String lastQuery = "";

    // Layout (computed in init)
    private int videoX, videoY, videoW, videoH;
    private int seekX, seekY, seekW;
    private int listX, listY, listW, listH;

    private Font font() {
        return Minecraft.getInstance().font;
    }

    private MediaController media() {
        return OverlayState.get().media;
    }

    @Override
    public void init() {
        Font font = font();
        int pad = 16;
        int top = y + pad;

        // Input row
        int inputW = Math.min(width - pad * 2 - 96, 560);
        input = new EditBox(font, x + pad, top, inputW, 20, Component.literal("YouTube URL or search"));
        input.setMaxLength(512);
        input.setHint(Component.literal("Paste a YouTube link or type to filter…"));
        input.setBordered(true);
        input.setVisible(true);
        input.setTextColor(OverlayTheme.TEXT);
        input.setValue(lastQuery);

        transport.clear();
        Ui.button(transport, "Play", this::playFromInput).accent = true;
        Ui.button(transport, "Refresh feed", this::refreshPool);
        Ui.layoutRow(transport, font, x + pad + inputW + 8, top, 20, 6);

        // Split: video left, feed right
        int midY = top + 28;
        int rightW = Math.max(220, (int) (width * 0.34));
        listX = x + width - pad - rightW;
        listY = midY + 16;
        listW = rightW;
        listH = height - (listY - y) - pad;

        videoX = x + pad;
        videoY = midY;
        videoW = Math.max(280, listX - pad - videoX);
        videoH = Math.max(160, videoW * 9 / 16);
        int maxVideoH = height - (midY - y) - TRANSPORT_H - SEEK_H - 64;
        if (videoH > maxVideoH) {
            videoH = Math.max(120, maxVideoH);
            videoW = videoH * 16 / 9;
        }

        seekX = videoX;
        seekY = videoY + videoH + 8;
        seekW = videoW;

        // Transport row under seek bar
        transport.clear();
        Ui.button(transport, "⏯", () -> media().toggle());
        Ui.button(transport, "⏹", () -> media().stop());
        Ui.button(transport, "Next", () -> media().next());
        Ui.button(transport, "Vol −", () -> media().volumeDown());
        Ui.button(transport, "Vol +", () -> media().volumeUp());
        Ui.layoutRow(transport, font, videoX, seekY + SEEK_H + 6, TRANSPORT_H, 6);

        // PiP settings row
        pipBar.clear();
        OverlayConfig cfg = OverlayConfig.get();
        Ui.button(pipBar, Ui.toggleLabel("PiP", cfg.pipEnabled), () -> {
            cfg.pipEnabled = !cfg.pipEnabled;
            cfg.save();
            init();
        });
        Ui.button(pipBar, "Corner", () -> {
            cfg.pipCorner = (cfg.pipCorner + 1) % 4;
            cfg.save();
        });
        Ui.button(pipBar, "Size", () -> {
            cfg.pipScale = (cfg.pipScale + 1) % 3;
            cfg.save();
        });
        Ui.layoutRow(pipBar, font, videoX + 240, seekY + SEEK_H + 6, TRANSPORT_H, 6);

        if (pool.isEmpty() && !poolLoading) {
            refreshPool();
        }
    }

    private void playFromInput() {
        String raw = input.getValue();
        String url = YoutubeFeed.normalizeUrl(raw);
        if (url.isEmpty()) {
            return;
        }
        if (url.startsWith("http")) {
            media().play(url, raw.startsWith("http") ? "" : raw);
            statusHint = "";
        } else {
            applyFilter();
        }
    }

    private void refreshPool() {
        if (OverlayState.get().session == null) {
            poolFailed = true;
            statusHint = "No overlay session — launch via TuffBox to browse the feed.";
            return;
        }
        poolLoading = true;
        poolFailed = false;
        Thread.startVirtualThread(() -> {
            List<YoutubeFeed.FeedVideo> fetched =
                    YoutubeFeed.fetchPool(OverlayState.get().session, 120);
            synchronized (YoutubePanel.this) {
                pool = fetched;
                poolLoading = false;
                poolFailed = fetched.isEmpty();
                applyFilter();
            }
        });
    }

    private synchronized void applyFilter() {
        filtered = YoutubeFeed.filter(pool, input == null ? "" : input.getValue());
        scroll = 0;
    }

    @Override
    public void render(GuiGraphics graphics, int mouseX, int mouseY, float partialTick) {
        Font font = font();
        graphics.fill(x, y, x + width, y + height, OverlayTheme.CONTENT_BG);

        input.render(graphics, mouseX, mouseY, partialTick);
        renderVideoArea(graphics, font, mouseX, mouseY);
        renderSeekBar(graphics, font, mouseX, mouseY);
        Ui.renderAll(graphics, font, transport, mouseX, mouseY);
        Ui.renderAll(graphics, font, pipBar, mouseX, mouseY);
        renderQueue(graphics, font);
        renderFeed(graphics, font, mouseX, mouseY);
    }

    private void renderVideoArea(GuiGraphics graphics, Font font, int mouseX, int mouseY) {
        graphics.fill(videoX, videoY, videoX + videoW, videoY + videoH, 0xFF000000);

        MediaController media = media();
        if (media.watermediaMissing()) {
            drawCentered(graphics, font, "WATERMeDIA is not installed", videoX, videoY, videoW, videoH, -14,
                    OverlayTheme.WARNING);
            drawCentered(graphics, font, "Relaunch from TuffBox to fetch it, or add it to the instance mods.",
                    videoX, videoY, videoW, videoH, 0, OverlayTheme.TEXT_DIM);
            return;
        }
        if (media.engineLoading()) {
            drawCentered(graphics, font, "Video engine is starting (first run extracts VLC)…",
                    videoX, videoY, videoW, videoH, -4, OverlayTheme.TEXT_DIM);
            return;
        }
        if (!media.isActive()) {
            drawCentered(graphics, font, "Nothing playing", videoX, videoY, videoW, videoH, -14,
                    OverlayTheme.TEXT_DIM);
            drawCentered(graphics, font, "Paste a link or pick a video from the feed →",
                    videoX, videoY, videoW, videoH, 0, OverlayTheme.TEXT_MUTED);
            return;
        }
        int tex = media.texture();
        if (tex >= 0 && media.videoWidth() > 0) {
            TextureBlit.drawFit(graphics, tex, videoX, videoY, videoW, videoH,
                    media.videoWidth(), media.videoHeight());
        } else {
            drawCentered(graphics, font, "Buffering…", videoX, videoY, videoW, videoH, -4,
                    OverlayTheme.TEXT_DIM);
        }

        // Title strip over the video
        String title = media.currentTitle();
        if (!title.isEmpty()) {
            graphics.fill(videoX, videoY, videoX + videoW, videoY + 14, 0x99000000);
            graphics.drawString(font, ellipsize(font, title, videoW - 12),
                    videoX + 6, videoY + 3, OverlayTheme.TEXT, false);
        }
    }

    private void renderSeekBar(GuiGraphics graphics, Font font, int mouseX, int mouseY) {
        MediaController media = media();
        long dur = media.durationMs();
        long time = media.timeMs();

        graphics.fill(seekX, seekY + 5, seekX + seekW, seekY + 8, OverlayTheme.PANEL_BG);
        if (dur > 0) {
            int filled = (int) (seekW * Math.min(1.0, (double) time / (double) dur));
            graphics.fill(seekX, seekY + 5, seekX + filled, seekY + 8, OverlayTheme.ACCENT);
        }
        String clock = Ui.clock(time) + " / " + (dur > 0 ? Ui.clock(dur) : "--:--");
        graphics.drawString(font, clock, seekX + seekW + 8, seekY + 3, OverlayTheme.TEXT_DIM, false);
    }

    private void renderQueue(GuiGraphics graphics, Font font) {
        MediaController media = media();
        int qy = seekY + SEEK_H + TRANSPORT_H + 12;
        String state = media.isPlaying() ? "▶ playing"
                : media.isPaused() ? "❚❚ paused"
                : media.isActive() ? "… loading"
                : "■ stopped";
        graphics.drawString(font, state, videoX, qy, OverlayTheme.TEXT_DIM, false);
        graphics.drawString(font, "Vol " + OverlayConfig.get().volume + "%",
                videoX + 90, qy, OverlayTheme.TEXT_DIM, false);
        if (media.queueSize() > 0) {
            String next = "Queue " + media.queueSize() + " — next: "
                    + ellipsize(font, media.queueTitleAt(0), videoW - 220);
            graphics.drawString(font, next, videoX + 160, qy, OverlayTheme.TEXT_MUTED, false);
        }
        if (!statusHint.isEmpty()) {
            graphics.drawString(font, statusHint, videoX, qy + 12, OverlayTheme.WARNING, false);
        }
    }

    private void renderFeed(GuiGraphics graphics, Font font, int mouseX, int mouseY) {
        graphics.fill(listX, listY - 14, listX + listW, listY + listH, OverlayTheme.PANEL_BG);
        graphics.drawString(font, "Minecraft feed", listX + 8, listY - 10, OverlayTheme.TEXT, false);

        List<YoutubeFeed.FeedVideo> rows = currentRows();
        int rowH = 30;
        int visible = Math.max(1, listH / rowH);
        int maxScroll = Math.max(0, rows.size() - visible);
        scroll = Math.max(0, Math.min(scroll, maxScroll));

        graphics.enableScissor(listX, listY, listX + listW, listY + listH);
        for (int i = scroll; i < rows.size() && (i - scroll) < visible; i++) {
            YoutubeFeed.FeedVideo v = rows.get(i);
            int ry = listY + (i - scroll) * rowH;
            boolean hovered = mouseX >= listX && mouseX < listX + listW
                    && mouseY >= ry && mouseY < ry + rowH;
            if (hovered) {
                graphics.fill(listX + 1, ry, listX + listW - 1, ry + rowH, OverlayTheme.RAIL_ITEM_HOVER);
            }
            graphics.fill(listX + 1, ry, listX + 4, ry + rowH, OverlayTheme.ACCENT);
            graphics.drawString(font, ellipsize(font, v.title, listW - 20), listX + 10, ry + 3,
                    OverlayTheme.TEXT, false);
            String meta = v.channel + " • " + YoutubeFeed.formatViews(v.views) + " views";
            graphics.drawString(font, ellipsize(font, meta, listW - 20), listX + 10, ry + 16,
                    OverlayTheme.TEXT_DIM, false);
        }
        graphics.disableScissor();

        if (poolLoading) {
            graphics.drawString(font, "Loading feed…", listX + 8, listY + 4, OverlayTheme.TEXT_DIM, false);
        } else if (poolFailed && rows.isEmpty()) {
            graphics.drawString(font, "Feed unavailable", listX + 8, listY + 4, OverlayTheme.WARNING, false);
        } else if (rows.isEmpty()) {
            graphics.drawString(font, "No matches", listX + 8, listY + 4, OverlayTheme.TEXT_DIM, false);
        }
    }

    private synchronized List<YoutubeFeed.FeedVideo> currentRows() {
        return filtered.isEmpty() && (input == null || input.getValue().isEmpty()) ? pool : filtered;
    }

    @Override
    public boolean mouseClicked(double mouseX, double mouseY, int button) {
        if (input.mouseClicked(mouseX, mouseY, button)) {
            input.setFocused(true);
            return true;
        }
        input.setFocused(false);

        if (Ui.click(transport, mouseX, mouseY) || Ui.click(pipBar, mouseX, mouseY)) {
            return true;
        }

        // Seek bar
        MediaController media = media();
        long dur = media.durationMs();
        if (dur > 0 && mouseY >= seekY && mouseY < seekY + SEEK_H
                && mouseX >= seekX && mouseX < seekX + seekW) {
            double frac = (mouseX - seekX) / (double) seekW;
            media.seekTo((long) (dur * frac));
            return true;
        }

        // Feed rows: left = play now, right = enqueue
        List<YoutubeFeed.FeedVideo> rows = currentRows();
        int rowH = 30;
        int visible = Math.max(1, listH / rowH);
        if (mouseX >= listX && mouseX < listX + listW && mouseY >= listY && mouseY < listY + listH) {
            int idx = scroll + (int) ((mouseY - listY) / rowH);
            if (idx >= 0 && idx < rows.size() && idx - scroll < visible) {
                YoutubeFeed.FeedVideo v = rows.get(idx);
                if (button == 1) {
                    media.enqueue(v.watchUrl(), v.title);
                } else {
                    media.play(v.watchUrl(), v.title);
                }
                return true;
            }
        }
        return false;
    }

    @Override
    public boolean mouseScrolled(double mouseX, double mouseY, double scrollX, double scrollY) {
        if (mouseX >= listX && mouseX < listX + listW && mouseY >= listY && mouseY < listY + listH) {
            scroll -= (int) scrollY;
            return true;
        }
        return false;
    }

    @Override
    public boolean keyPressed(int keyCode, int scanCode, int modifiers) {
        if (input.isFocused()) {
            if (keyCode == 257) { // Enter
                playFromInput();
                return true;
            }
            if (keyCode == 256) { // Esc → unfocus, let screen handle
                input.setFocused(false);
                return true;
            }
            boolean handled = input.keyPressed(keyCode, scanCode, modifiers);
            if (handled && !input.getValue().equals(lastQuery)) {
                lastQuery = input.getValue();
                applyFilter();
            }
            return handled;
        }
        return false;
    }

    @Override
    public boolean charTyped(char codePoint, int modifiers) {
        if (input.isFocused() && input.charTyped(codePoint, modifiers)) {
            lastQuery = input.getValue();
            applyFilter();
            return true;
        }
        return false;
    }

    private static String ellipsize(Font font, String text, int maxWidth) {
        if (text == null) {
            return "";
        }
        if (font.width(text) <= maxWidth) {
            return text;
        }
        String ell = "…";
        while (!text.isEmpty() && font.width(text + ell) > maxWidth) {
            text = text.substring(0, text.length() - 1);
        }
        return text + ell;
    }

    private void drawCentered(GuiGraphics graphics, Font font, String text,
                              int bx, int by, int bw, int bh, int dy, int color) {
        graphics.drawString(font, text, bx + (bw - font.width(text)) / 2, by + bh / 2 + dy, color, false);
    }
}
