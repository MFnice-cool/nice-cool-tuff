package dev.tuffbox.overlay;

import dev.tuffbox.overlay.panel.ChatPanel;
import dev.tuffbox.overlay.panel.FriendsPanel;
import dev.tuffbox.overlay.panel.Panel;
import dev.tuffbox.overlay.panel.YoutubePanel;
import net.minecraft.client.gui.GuiGraphics;
import net.minecraft.client.gui.screens.Screen;
import net.minecraft.network.chat.Component;

import java.time.LocalTime;
import java.time.format.DateTimeFormatter;

/**
 * TuffBox in-game overlay shell (Discord/Steam style): left app rail,
 * top bar, swappable content panel. Open with the overlay keybind (F8).
 * Never pauses singleplayer — the game keeps running behind the backdrop.
 */
public final class OverlayScreen extends Screen {
    public static final int PAGE_YOUTUBE = 0;
    public static final int PAGE_FRIENDS = 1;
    public static final int PAGE_CHAT = 2;

    private static final String[] RAIL_LABELS = {"YouTube", "Friends", "Chat"};

    private final Screen parent;
    private final YoutubePanel youtubePanel = new YoutubePanel();
    private final FriendsPanel friendsPanel = new FriendsPanel();
    private final ChatPanel chatPanel = new ChatPanel();

    private int page;
    private int contentX;
    private int contentY;
    private int contentW;
    private int contentH;

    public OverlayScreen(Screen parent) {
        super(Component.literal("TuffBox Overlay"));
        this.parent = parent;
        this.page = clampPage(OverlayConfig.get().lastPage);
    }

    @Override
    protected void init() {
        contentX = OverlayTheme.RAIL_WIDTH;
        contentY = OverlayTheme.TOPBAR_HEIGHT;
        contentW = Math.max(0, width - contentX);
        contentH = Math.max(0, height - contentY);
        for (Panel panel : allPanels()) {
            panel.attach(this);
        }
        layoutPanels();
    }

    private void layoutPanels() {
        for (Panel panel : allPanels()) {
            panel.setBounds(contentX, contentY, contentW, contentH);
            panel.init();
        }
    }

    /** Friends → Chat deep link: open the DM with this peer. */
    public void openChat(String peerKey, String peerName) {
        switchPage(PAGE_CHAT);
        chatPanel.openConversation(peerKey, peerName);
    }

    private Panel[] allPanels() {
        return new Panel[]{youtubePanel, friendsPanel, chatPanel};
    }

    private Panel activePanel() {
        switch (page) {
            case PAGE_FRIENDS:
                return friendsPanel;
            case PAGE_CHAT:
                return chatPanel;
            default:
                return youtubePanel;
        }
    }

    @Override
    public boolean isPauseScreen() {
        return false;
    }

    @Override
    public void render(GuiGraphics graphics, int mouseX, int mouseY, float partialTick) {
        graphics.fill(0, 0, width, height, OverlayTheme.BACKDROP);

        activePanel().render(graphics, mouseX, mouseY, partialTick);
        renderRail(graphics, mouseX, mouseY);
        renderTopBar(graphics);
    }

    private void renderRail(GuiGraphics graphics, int mouseX, int mouseY) {
        int railW = OverlayTheme.RAIL_WIDTH;
        graphics.fill(0, 0, railW, height, OverlayTheme.RAIL_BG);
        graphics.fill(railW - 1, OverlayTheme.TOPBAR_HEIGHT, railW, height, OverlayTheme.DIVIDER);

        graphics.drawString(font, Component.literal("TuffBox"), 16, 16, OverlayTheme.TEXT, false);
        graphics.drawString(font, Component.literal("Overlay"), 16, 26, OverlayTheme.TEXT_MUTED, false);

        int itemY = OverlayTheme.TOPBAR_HEIGHT + 12;
        int itemH = 32;
        for (int i = 0; i < RAIL_LABELS.length; i++) {
            boolean active = i == page;
            boolean hovered = mouseX >= 8 && mouseX < railW - 8
                    && mouseY >= itemY && mouseY < itemY + itemH;
            if (active) {
                graphics.fill(8, itemY, railW - 8, itemY + itemH, OverlayTheme.RAIL_ITEM_ACTIVE);
                graphics.fill(8, itemY + 6, 11, itemY + itemH - 6, OverlayTheme.ACCENT);
            } else if (hovered) {
                graphics.fill(8, itemY, railW - 8, itemY + itemH, OverlayTheme.RAIL_ITEM_HOVER);
            }
            int color = active ? OverlayTheme.TEXT : OverlayTheme.TEXT_DIM;
            graphics.drawString(font, Component.literal(RAIL_LABELS[i]), 20, itemY + 12, color, false);
            if (i == PAGE_CHAT) {
                int unread = OverlayState.get().totalUnread();
                if (unread > 0) {
                    String badge = unread > 99 ? "99+" : String.valueOf(unread);
                    int bw = font.width(badge) + 8;
                    int bx = railW - 16 - bw;
                    int by = itemY + 8;
                    graphics.fill(bx, by, bx + bw, by + 16, OverlayTheme.DANGER);
                    graphics.drawString(font, badge, bx + 4, by + 4, OverlayTheme.TEXT, false);
                }
            }
            itemY += itemH + 4;
        }

        String hint = "F8 — close";
        graphics.drawString(font, hint, 16, height - 16, OverlayTheme.TEXT_MUTED, false);
        if (OverlayState.get().hasActiveMedia()) {
            graphics.drawString(font, "♪ playing in background", 16, height - 28,
                    OverlayTheme.SUCCESS, false);
        }
    }

    private void renderTopBar(GuiGraphics graphics) {
        int railW = OverlayTheme.RAIL_WIDTH;
        graphics.fill(railW, 0, width, OverlayTheme.TOPBAR_HEIGHT, OverlayTheme.TOPBAR_BG);
        graphics.fill(railW, OverlayTheme.TOPBAR_HEIGHT - 1, width, OverlayTheme.TOPBAR_HEIGHT,
                OverlayTheme.DIVIDER);

        graphics.drawString(font, Component.literal(RAIL_LABELS[page]),
                railW + 16, 16, OverlayTheme.TEXT, false);

        String clock = LocalTime.now().format(DateTimeFormatter.ofPattern("HH:mm"));
        graphics.drawString(font, clock, width - 16 - font.width(clock), 16,
                OverlayTheme.TEXT_DIM, false);

        var mc = net.minecraft.client.Minecraft.getInstance();
        if (mc.player != null) {
            String name = mc.player.getGameProfile().getName();
            if (name != null && !name.isEmpty()) {
                String label = name + "  •";
                graphics.drawString(font, label,
                        width - 24 - font.width(clock) - font.width(label), 16,
                        OverlayTheme.TEXT_DIM, false);
            }
        }
    }

    private void switchPage(int next) {
        next = clampPage(next);
        if (next == page) {
            return;
        }
        activePanel().onHide();
        page = next;
        OverlayConfig.get().lastPage = next;
        OverlayConfig.get().save();
        if (next == PAGE_CHAT) {
            // Reading chat clears badges as messages render; nothing global here.
        }
        activePanel().init();
    }

    private static int clampPage(int p) {
        return p < 0 ? 0 : (p > 2 ? 2 : p);
    }

    @Override
    public boolean mouseClicked(double mouseX, double mouseY, int button) {
        int railW = OverlayTheme.RAIL_WIDTH;
        if (mouseX < railW) {
            int itemY = OverlayTheme.TOPBAR_HEIGHT + 12;
            int itemH = 32;
            for (int i = 0; i < RAIL_LABELS.length; i++) {
                if (mouseX >= 8 && mouseX < railW - 8 && mouseY >= itemY && mouseY < itemY + itemH) {
                    switchPage(i);
                    return true;
                }
                itemY += itemH + 4;
            }
            return true;
        }
        return activePanel().mouseClicked(mouseX, mouseY, button) || super.mouseClicked(mouseX, mouseY, button);
    }

    @Override
    public boolean mouseReleased(double mouseX, double mouseY, int button) {
        return activePanel().mouseReleased(mouseX, mouseY, button) || super.mouseReleased(mouseX, mouseY, button);
    }

    @Override
    public boolean mouseDragged(double mouseX, double mouseY, int button, double dragX, double dragY) {
        return activePanel().mouseDragged(mouseX, mouseY, button, dragX, dragY)
                || super.mouseDragged(mouseX, mouseY, button, dragX, dragY);
    }

    @Override
    public boolean mouseScrolled(double mouseX, double mouseY, double scrollX, double scrollY) {
        return activePanel().mouseScrolled(mouseX, mouseY, scrollX, scrollY)
                || super.mouseScrolled(mouseX, mouseY, scrollX, scrollY);
    }

    @Override
    public boolean keyPressed(int keyCode, int scanCode, int modifiers) {
        if (OverlayRuntime.matchesOpenKey(keyCode, scanCode)) {
            onClose();
            return true;
        }
        return activePanel().keyPressed(keyCode, scanCode, modifiers)
                || super.keyPressed(keyCode, scanCode, modifiers);
    }

    @Override
    public boolean charTyped(char codePoint, int modifiers) {
        return activePanel().charTyped(codePoint, modifiers) || super.charTyped(codePoint, modifiers);
    }

    @Override
    public void tick() {
        activePanel().tick();
    }

    @Override
    public void onClose() {
        activePanel().onHide();
        minecraft.setScreen(parent);
    }
}
