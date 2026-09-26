package dev.tuffbox.overlay.widgets;

import dev.tuffbox.overlay.OverlayTheme;
import net.minecraft.client.gui.Font;
import net.minecraft.client.gui.GuiGraphics;

import java.util.ArrayList;
import java.util.List;

/**
 * Tiny immediate-mode button row used by overlay panels (vanilla widgets
 * are screen-scoped; panels render inside our own content area instead).
 */
public final class Ui {
    private Ui() {}

    public static final class Button {
        public int x, y, w, h;
        public String label;
        public Runnable action;
        public boolean enabled = true;
        public boolean accent;

        public Button(String label, Runnable action) {
            this.label = label;
            this.action = action;
        }
    }

    public static Button button(List<Button> bar, String label, Runnable action) {
        Button b = new Button(label, action);
        bar.add(b);
        return b;
    }

    /** Lay out a left-to-right button row with fixed heights and padding. */
    public static void layoutRow(List<Button> bar, Font font, int x, int y, int h, int gap) {
        int cx = x;
        for (Button b : bar) {
            b.w = font.width(b.label) + 16;
            b.h = h;
            b.x = cx;
            b.y = y;
            cx += b.w + gap;
        }
    }

    public static void render(GuiGraphics graphics, Font font, Button b, int mouseX, int mouseY) {
        boolean hovered = b.enabled && mouseX >= b.x && mouseX < b.x + b.w
                && mouseY >= b.y && mouseY < b.y + b.h;
        int bg = !b.enabled ? OverlayTheme.PANEL_BG
                : b.accent ? (hovered ? OverlayTheme.ACCENT_HOVER : OverlayTheme.ACCENT)
                : (hovered ? OverlayTheme.RAIL_ITEM_ACTIVE : OverlayTheme.RAIL_ITEM_HOVER);
        graphics.fill(b.x, b.y, b.x + b.w, b.y + b.h, bg);
        int color = b.enabled ? OverlayTheme.TEXT : OverlayTheme.TEXT_MUTED;
        graphics.drawString(font, b.label, b.x + 8, b.y + (b.h - 8) / 2, color, false);
    }

    public static void renderAll(GuiGraphics graphics, Font font, List<Button> bar,
                                 int mouseX, int mouseY) {
        for (Button b : bar) {
            render(graphics, font, b, mouseX, mouseY);
        }
    }

    public static boolean click(List<Button> bar, double mouseX, double mouseY) {
        for (Button b : bar) {
            if (b.enabled && mouseX >= b.x && mouseX < b.x + b.w
                    && mouseY >= b.y && mouseY < b.y + b.h) {
                b.action.run();
                return true;
            }
        }
        return false;
    }

    /** Toggle-style label helper: "Name: on/off". */
    public static String toggleLabel(String name, boolean on) {
        return name + ": " + (on ? "ON" : "off");
    }

    /** Word-wrap into drawString-sized lines. */
    public static List<String> wrap(Font font, String text, int maxWidth) {
        List<String> lines = new ArrayList<String>();
        if (text == null || text.isEmpty()) {
            return lines;
        }
        StringBuilder line = new StringBuilder();
        for (String word : text.split(" ")) {
            String candidate = line.length() == 0 ? word : line + " " + word;
            if (font.width(candidate) > maxWidth && line.length() > 0) {
                lines.add(line.toString());
                line = new StringBuilder(word);
            } else {
                line = new StringBuilder(candidate);
            }
        }
        if (line.length() > 0) {
            lines.add(line.toString());
        }
        return lines;
    }

    /** mm:ss / h:mm:ss for media clocks. */
    public static String clock(long ms) {
        long total = Math.max(0L, ms / 1000L);
        long h = total / 3600L;
        long m = (total % 3600L) / 60L;
        long s = total % 60L;
        if (h > 0) {
            return h + ":" + pad(m) + ":" + pad(s);
        }
        return m + ":" + pad(s);
    }

    private static String pad(long v) {
        return v < 10 ? "0" + v : Long.toString(v);
    }
}
