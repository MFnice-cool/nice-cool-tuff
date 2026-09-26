package dev.tuffbox.overlay.panel;

import dev.tuffbox.overlay.OverlayState;
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
 * Discord-style DM page: conversation list on the left (friends + unread
 * badges), message history on the right, input at the bottom.
 */
public final class ChatPanel extends Panel {
    private static final int LIST_W = 220;
    private static final int ROW_H = 30;

    private EditBox input;
    private String peerKey = "";
    private String peerName = "";
    private int listScroll;
    private int msgScroll;
    /** Messages are newest-at-bottom; msgScroll=0 means pinned to bottom. */

    private int msgX, msgY, msgW, msgH;
    private final List<Ui.Button> sendBar = new ArrayList<Ui.Button>();

    private Font font() {
        return Minecraft.getInstance().font;
    }

    public void openConversation(String key, String name) {
        peerKey = key == null ? "" : key;
        peerName = name == null ? "" : name;
        msgScroll = 0;
        SocialService.get().setOpenConversation(peerKey);
    }

    @Override
    public void init() {
        Font font = font();
        int pad = 12;

        int inputY = y + height - pad - 20;
        int inputX = x + LIST_W + pad;
        int inputW = x + width - pad - inputX - 74;

        input = new EditBox(font, inputX, inputY, Math.max(120, inputW), 20,
                Component.literal("Message"));
        input.setMaxLength(500);
        input.setBordered(true);
        input.setVisible(true);
        input.setTextColor(OverlayTheme.TEXT);
        input.setHint(Component.literal(peerKey.isEmpty() ? "Pick a conversation…"
                : "Message " + peerName));

        sendBar.clear();
        Ui.button(sendBar, "Send", this::sendCurrent).accent = true;
        Ui.layoutRow(sendBar, font, x + width - pad - 62, inputY, 20, 6);

        msgX = inputX;
        msgY = y + pad + 12;
        msgW = x + width - pad - msgX;
        msgH = inputY - msgY - 8;
    }

    @Override
    public void onHide() {
        SocialService.get().setOpenConversation("");
    }

    private void sendCurrent() {
        String text = input.getValue().trim();
        if (text.isEmpty() || peerKey.isEmpty()) {
            return;
        }
        input.setValue("");
        SocialService.get().sendAsync(peerKey, text, () -> msgScroll = 0);
    }

    @Override
    public void render(GuiGraphics graphics, int mouseX, int mouseY, float partialTick) {
        Font font = font();
        graphics.fill(x, y, x + width, y + height, OverlayTheme.CONTENT_BG);

        if (!SocialService.get().available()) {
            graphics.drawCenteredString(font, Component.literal("Chat needs a TuffBox session"),
                    x + width / 2, y + height / 2 - 4, OverlayTheme.TEXT_DIM);
            return;
        }

        renderConversationList(graphics, font, mouseX, mouseY);
        renderMessages(graphics, font, mouseX, mouseY);
        input.render(graphics, mouseX, mouseY, partialTick);
        Ui.renderAll(graphics, font, sendBar, mouseX, mouseY);
    }

    private void renderConversationList(GuiGraphics graphics, Font font, int mouseX, int mouseY) {
        graphics.fill(x, y, x + LIST_W, y + height, OverlayTheme.PANEL_BG);
        graphics.fill(x + LIST_W - 1, y, x + LIST_W, y + height, OverlayTheme.DIVIDER);
        graphics.drawString(font, "Direct messages", x + 10, y + 12, OverlayTheme.TEXT_MUTED, false);

        List<SocialApi.Friend> rows = SocialService.get().friends().friends;
        int top = y + 30;
        int visible = Math.max(1, (height - 40) / ROW_H);
        int maxScroll = Math.max(0, rows.size() - visible);
        listScroll = Math.max(0, Math.min(listScroll, maxScroll));

        for (int i = listScroll; i < rows.size() && (i - listScroll) < visible; i++) {
            SocialApi.Friend f = rows.get(i);
            int ry = top + (i - listScroll) * ROW_H;
            boolean selected = f.key.equals(peerKey);
            boolean hovered = mouseX >= x && mouseX < x + LIST_W - 1 && mouseY >= ry && mouseY < ry + ROW_H;
            if (selected) {
                graphics.fill(x + 4, ry, x + LIST_W - 5, ry + ROW_H, OverlayTheme.RAIL_ITEM_ACTIVE);
            } else if (hovered) {
                graphics.fill(x + 4, ry, x + LIST_W - 5, ry + ROW_H, OverlayTheme.RAIL_ITEM_HOVER);
            }
            int dot = f.online ? OverlayTheme.SUCCESS : OverlayTheme.TEXT_MUTED;
            graphics.fill(x + 12, ry + 11, x + 18, ry + 17, dot);
            graphics.drawString(font, clip(font, f.name, LIST_W - 78), x + 24, ry + 10,
                    selected ? OverlayTheme.TEXT : OverlayTheme.TEXT_DIM, false);

            int unread = OverlayState.get().unread.getOrDefault(f.key, 0);
            if (unread > 0 && !selected) {
                String badge = unread > 99 ? "99+" : String.valueOf(unread);
                int bw = font.width(badge) + 8;
                int bx = x + LIST_W - 14 - bw;
                graphics.fill(bx, ry + 7, bx + bw, ry + 7 + 14, OverlayTheme.DANGER);
                graphics.drawString(font, badge, bx + 4, ry + 10, OverlayTheme.TEXT, false);
            }
        }
        if (rows.isEmpty()) {
            graphics.drawString(font, "No friends yet.", x + 10, top + 6, OverlayTheme.TEXT_MUTED, false);
            graphics.drawString(font, "Add some on the Friends tab.", x + 10, top + 18,
                    OverlayTheme.TEXT_MUTED, false);
        }
    }

    private void renderMessages(GuiGraphics graphics, Font font, int mouseX, int mouseY) {
        if (peerKey.isEmpty()) {
            graphics.drawCenteredString(font, Component.literal("Select a friend to start chatting"),
                    msgX + msgW / 2, msgY + msgH / 2 - 4, OverlayTheme.TEXT_MUTED);
            return;
        }
        graphics.drawString(font, "@ " + peerName, msgX, y + 10, OverlayTheme.TEXT, false);
        graphics.fill(msgX, msgY - 4, msgX + msgW, msgY - 3, OverlayTheme.DIVIDER);

        List<SocialApi.ChatMessage> msgs = SocialService.get().conversationWith(peerKey);
        String myKey = OverlayState.get().session != null ? OverlayState.get().session.uuid : "";

        // Flatten to render lines (header + wrapped body), pinned to bottom;
        // msgScroll shifts the viewport upwards into history.
        int lineW = msgW - 24;
        int lineH = 11;
        List<String> texts = new ArrayList<String>();
        List<Integer> colors = new ArrayList<Integer>();
        for (SocialApi.ChatMessage m : msgs) {
            boolean mine = m.fromKey.equals(myKey);
            texts.add(m.fromName);
            colors.add(mine ? OverlayTheme.ACCENT : OverlayTheme.SUCCESS);
            for (String bodyLine : Ui.wrap(font, m.body, lineW)) {
                texts.add(bodyLine);
                colors.add(OverlayTheme.TEXT);
            }
        }

        int totalLines = texts.size();
        int visibleLines = Math.max(1, msgH / lineH);
        int maxScroll = Math.max(0, totalLines - visibleLines);
        msgScroll = Math.max(0, Math.min(msgScroll, maxScroll));

        int firstVisible = Math.max(0, totalLines - visibleLines - msgScroll);
        graphics.enableScissor(msgX, msgY, msgX + msgW, msgY + msgH);
        for (int i = 0; i < visibleLines && firstVisible + i < totalLines; i++) {
            int idx = firstVisible + i;
            graphics.drawString(font, texts.get(idx), msgX + 10, msgY + 4 + i * lineH,
                    colors.get(idx), false);
        }
        graphics.disableScissor();

        if (msgs.isEmpty()) {
            graphics.drawCenteredString(font, Component.literal("No messages yet — say hi!"),
                    msgX + msgW / 2, msgY + msgH / 2 - 4, OverlayTheme.TEXT_MUTED);
        }
    }

    @Override
    public boolean mouseClicked(double mouseX, double mouseY, int button) {
        if (input.mouseClicked(mouseX, mouseY, button)) {
            input.setFocused(true);
            return true;
        }
        input.setFocused(false);
        if (Ui.click(sendBar, mouseX, mouseY)) {
            return true;
        }

        // Conversation list rows
        if (mouseX >= x && mouseX < x + LIST_W && mouseY >= y + 30 && mouseY < y + height) {
            List<SocialApi.Friend> rows = SocialService.get().friends().friends;
            int idx = listScroll + (int) ((mouseY - (y + 30)) / ROW_H);
            if (idx >= 0 && idx < rows.size()) {
                SocialApi.Friend f = rows.get(idx);
                openConversation(f.key, f.name);
                input.setHint(Component.literal("Message " + peerName));
                return true;
            }
        }
        return false;
    }

    @Override
    public boolean mouseScrolled(double mouseX, double mouseY, double scrollX, double scrollY) {
        if (mouseX >= msgX && mouseX < msgX + msgW && mouseY >= msgY && mouseY < msgY + msgH) {
            msgScroll -= (int) scrollY;
            msgScroll = Math.max(0, msgScroll);
            return true;
        }
        if (mouseX >= x && mouseX < x + LIST_W) {
            listScroll -= (int) scrollY;
            return true;
        }
        return false;
    }

    @Override
    public boolean keyPressed(int keyCode, int scanCode, int modifiers) {
        if (input.isFocused()) {
            if (keyCode == 257) {
                sendCurrent();
                return true;
            }
            if (keyCode == 256) {
                input.setFocused(false);
                return true;
            }
            return input.keyPressed(keyCode, scanCode, modifiers);
        }
        return false;
    }

    @Override
    public boolean charTyped(char codePoint, int modifiers) {
        return input.isFocused() && input.charTyped(codePoint, modifiers);
    }

    private static String clip(Font font, String s, int maxW) {
        if (font.width(s) <= maxW) {
            return s;
        }
        while (s.length() > 1 && font.width(s + "…") > maxW) {
            s = s.substring(0, s.length() - 1);
        }
        return s + "…";
    }
}
