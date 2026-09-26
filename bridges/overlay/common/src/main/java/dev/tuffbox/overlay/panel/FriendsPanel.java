package dev.tuffbox.overlay.panel;

import dev.tuffbox.overlay.OverlayConfig;
import dev.tuffbox.overlay.OverlayTheme;
import dev.tuffbox.overlay.core.SocialApi;
import dev.tuffbox.overlay.social.SocialService;
import dev.tuffbox.overlay.widgets.Ui;
import net.minecraft.client.Minecraft;
import net.minecraft.client.gui.Font;
import net.minecraft.client.gui.GuiGraphics;
import net.minecraft.client.gui.components.EditBox;
import net.minecraft.network.chat.Component;

import java.util.ArrayList;
import java.util.List;

/**
 * Discord-style friends page: add by username, incoming requests with
 * accept/decline, online presence (pack + server) per friend, jump to chat.
 */
public final class FriendsPanel extends Panel {
    private EditBox addInput;
    private final List<Ui.Button> topBar = new ArrayList<Ui.Button>();
    private String hint = "";
    private long hintColor = OverlayTheme.TEXT_DIM;
    private int scroll;

    // Row layout constants
    private static final int ROW_H = 34;
    private int listX, listY, listW, listH;

    private Font font() {
        return Minecraft.getInstance().font;
    }

    @Override
    public void init() {
        Font font = font();
        int pad = 16;
        int top = y + pad;

        addInput = new EditBox(font, x + pad, top, 240, 20, Component.literal("Add friend"));
        addInput.setMaxLength(32);
        addInput.setHint(Component.literal("Add friend by username…"));
        addInput.setBordered(true);
        addInput.setVisible(true);
        addInput.setTextColor(OverlayTheme.TEXT);

        topBar.clear();
        Ui.button(topBar, "Send request", this::sendRequest).accent = true;
        Ui.button(topBar, "Refresh", () -> SocialService.get().refreshFriendsAsync());
        Ui.button(topBar, Ui.toggleLabel("Presence", OverlayConfig.get().presenceOptIn), () -> {
            OverlayConfig.get().presenceOptIn = !OverlayConfig.get().presenceOptIn;
            OverlayConfig.get().save();
            init();
        });
        Ui.layoutRow(topBar, font, x + pad + 248, top, 20, 8);

        listX = x + pad;
        listY = top + 34;
        listW = width - pad * 2;
        listH = height - (listY - y) - pad;
    }

    private void sendRequest() {
        String name = addInput.getValue().trim();
        if (name.isEmpty()) {
            return;
        }
        addInput.setValue("");
        hint = "Sending…";
        hintColor = OverlayTheme.TEXT_DIM;
        Thread.startVirtualThread(() -> {
            String result = SocialApi.addFriend(
                    dev.tuffbox.overlay.OverlayState.get().session, name);
            if (result == null) {
                hint = "Player not found (they must launch via TuffBox at least once).";
                hintColor = OverlayTheme.WARNING;
            } else if (result.equals("accepted")) {
                hint = name + " accepted your request — you are friends now.";
                hintColor = OverlayTheme.SUCCESS;
                SocialService.get().refreshFriendsAsync();
            } else if (result.startsWith("already")) {
                hint = "Already " + result.substring("already:".length()) + ".";
                hintColor = OverlayTheme.TEXT_DIM;
            } else {
                hint = "Request sent to " + name + ".";
                hintColor = OverlayTheme.SUCCESS;
                SocialService.get().refreshFriendsAsync();
            }
        });
    }

    @Override
    public void render(GuiGraphics graphics, int mouseX, int mouseY, float partialTick) {
        Font font = font();
        graphics.fill(x, y, x + width, y + height, OverlayTheme.CONTENT_BG);

        if (!SocialService.get().available()) {
            graphics.drawCenteredString(font, Component.literal("Friends need a TuffBox session"),
                    x + width / 2, y + height / 2 - 14, OverlayTheme.TEXT_DIM);
            graphics.drawCenteredString(font,
                    Component.literal("Launch the game from the TuffBox launcher to sign in."),
                    x + width / 2, y + height / 2, OverlayTheme.TEXT_MUTED);
            return;
        }

        addInput.render(graphics, mouseX, mouseY, partialTick);
        Ui.renderAll(graphics, font, topBar, mouseX, mouseY);
        if (!hint.isEmpty()) {
            graphics.drawString(font, hint, x + 16, y + 16 + 26, (int) hintColor, false);
        }

        SocialApi.FriendsSnapshot snap = SocialService.get().friends();

        // Build a flat row model: section headers + friend rows.
        List<Object> rows = new ArrayList<Object>();
        if (!snap.incoming.isEmpty()) {
            rows.add("Requests (" + snap.incoming.size() + ")");
            rows.addAll(snap.incoming);
        }
        rows.add("Friends (" + snap.friends.size() + ")");
        rows.addAll(snap.friends);
        if (!snap.outgoing.isEmpty()) {
            rows.add("Outgoing");
            rows.addAll(snap.outgoing);
        }

        int visible = Math.max(1, listH / ROW_H);
        int maxScroll = Math.max(0, rows.size() - visible);
        scroll = Math.max(0, Math.min(scroll, maxScroll));

        graphics.enableScissor(listX, listY, listX + listW, listY + listH);
        for (int i = scroll; i < rows.size() && (i - scroll) < visible; i++) {
            int ry = listY + (i - scroll) * ROW_H;
            Object row = rows.get(i);
            if (row instanceof String) {
                graphics.drawString(font, (String) row, listX + 4, ry + 12,
                        OverlayTheme.TEXT_MUTED, false);
                continue;
            }
            SocialApi.Friend f = (SocialApi.Friend) row;
            boolean hovered = mouseX >= listX && mouseX < listX + listW
                    && mouseY >= ry && mouseY < ry + ROW_H;
            if (hovered) {
                graphics.fill(listX, ry, listX + listW, ry + ROW_H, OverlayTheme.RAIL_ITEM_HOVER);
            }

            // Presence dot
            int dotColor = f.online ? OverlayTheme.SUCCESS : OverlayTheme.TEXT_MUTED;
            graphics.fill(listX + 6, ry + 10, listX + 14, ry + 18, dotColor);

            graphics.drawString(font, f.name, listX + 22, ry + 5, OverlayTheme.TEXT, false);
            String sub;
            if (f.online) {
                sub = f.pack.isEmpty() ? "Online" : "Playing " + f.pack;
                if (!f.server.isEmpty()) {
                    sub += " • " + f.server;
                }
            } else {
                sub = "Offline";
            }
            graphics.drawString(font, ellipsize(font, sub, listW - 260), listX + 22, ry + 19,
                    f.online ? OverlayTheme.TEXT_DIM : OverlayTheme.TEXT_MUTED, false);

            // Row actions: [Chat] [Remove] for friends; [Accept] [Decline] for requests
            int bx = listX + listW - 8;
            if (snap.incoming.contains(f)) {
                bx = drawRowButton(graphics, font, "✕", bx, ry, mouseX, mouseY);
                bx = drawRowButton(graphics, font, "✓", bx, ry, mouseX, mouseY);
            } else {
                bx = drawRowButton(graphics, font, "✕", bx, ry, mouseX, mouseY);
                if (snap.friends.contains(f)) {
                    bx = drawRowButton(graphics, font, "Chat", bx, ry, mouseX, mouseY);
                }
            }
        }
        graphics.disableScissor();
    }

    /** Returns new left edge (x decreases as buttons are added right-to-left). */
    private int drawRowButton(GuiGraphics graphics, Font font, String label,
                              int rightEdge, int ry, int mouseX, int mouseY) {
        int w = font.width(label) + 12;
        int bx = rightEdge - w;
        int by = ry + 8;
        boolean hovered = mouseX >= bx && mouseX < bx + w && mouseY >= by && mouseY < by + 18;
        graphics.fill(bx, by, bx + w, by + 18,
                hovered ? OverlayTheme.ACCENT_HOVER : OverlayTheme.PANEL_BG);
        graphics.drawString(font, label, bx + 6, by + 5, OverlayTheme.TEXT, false);
        return bx - 6;
    }

    @Override
    public boolean mouseClicked(double mouseX, double mouseY, int button) {
        if (addInput.mouseClicked(mouseX, mouseY, button)) {
            addInput.setFocused(true);
            return true;
        }
        addInput.setFocused(false);
        if (Ui.click(topBar, mouseX, mouseY)) {
            return true;
        }

        SocialApi.FriendsSnapshot snap = SocialService.get().friends();
        List<Object> rows = new ArrayList<Object>();
        if (!snap.incoming.isEmpty()) {
            rows.add("Requests (" + snap.incoming.size() + ")");
            rows.addAll(snap.incoming);
        }
        rows.add("Friends (" + snap.friends.size() + ")");
        rows.addAll(snap.friends);
        if (!snap.outgoing.isEmpty()) {
            rows.add("Outgoing");
            rows.addAll(snap.outgoing);
        }

        if (mouseX >= listX && mouseX < listX + listW && mouseY >= listY && mouseY < listY + listH) {
            int idx = scroll + (int) ((mouseY - listY) / ROW_H);
            if (idx >= 0 && idx < rows.size()) {
                Object row = rows.get(idx);
                if (row instanceof SocialApi.Friend) {
                    handleRowClick((SocialApi.Friend) row, mouseX, mouseY);
                    return true;
                }
            }
        }
        return false;
    }

    private void handleRowClick(SocialApi.Friend f, double mouseX, double mouseY) {
        SocialApi.FriendsSnapshot snap = SocialService.get().friends();
        Font font = font();

        java.util.List<String> labels = new ArrayList<String>();
        if (snap.incoming.contains(f)) {
            labels.add("✕");
            labels.add("✓");
        } else {
            labels.add("✕");
            if (snap.friends.contains(f)) {
                labels.add("Chat");
            }
        }
        // Row top from the click position (rows are ROW_H, starting at scroll).
        int rowTop = listY + ((int) (mouseY - listY) / ROW_H) * ROW_H;
        int by = rowTop + 8;
        int edge = listX + listW - 8;
        for (String label : labels) {
            int w = font.width(label) + 12;
            int bx = edge - w;
            if (mouseX >= bx && mouseX < bx + w && mouseY >= by && mouseY < by + 18) {
                dispatchRowAction(f, label);
                return;
            }
            edge = bx - 6;
        }
    }

    private void dispatchRowAction(SocialApi.Friend f, String label) {
        if (label.equals("Chat")) {
            if (screen != null) {
                screen.openChat(f.key, f.name);
            }
            return;
        }
        if (label.equals("✓")) {
            Thread.startVirtualThread(() -> {
                SocialApi.acceptFriend(dev.tuffbox.overlay.OverlayState.get().session, f.id);
                SocialService.get().refreshFriendsAsync();
            });
            return;
        }
        // ✕ — remove/decline
        Thread.startVirtualThread(() -> {
            SocialApi.removeFriend(dev.tuffbox.overlay.OverlayState.get().session, f.id);
            SocialService.get().refreshFriendsAsync();
        });
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
        if (addInput.isFocused()) {
            if (keyCode == 257) {
                sendRequest();
                return true;
            }
            if (keyCode == 256) {
                addInput.setFocused(false);
                return true;
            }
            return addInput.keyPressed(keyCode, scanCode, modifiers);
        }
        return false;
    }

    @Override
    public boolean charTyped(char codePoint, int modifiers) {
        return addInput.isFocused() && addInput.charTyped(codePoint, modifiers);
    }

    private static String ellipsize(Font font, String text, int maxWidth) {
        if (font.width(text) <= maxWidth) {
            return text;
        }
        while (!text.isEmpty() && font.width(text + "…") > maxWidth) {
            text = text.substring(0, text.length() - 1);
        }
        return text + "…";
    }
}
