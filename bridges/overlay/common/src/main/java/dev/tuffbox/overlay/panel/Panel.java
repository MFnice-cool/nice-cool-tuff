package dev.tuffbox.overlay.panel;

import dev.tuffbox.overlay.OverlayScreen;
import net.minecraft.client.gui.GuiGraphics;

/**
 * Content-area panel hosted by OverlayScreen. The screen forwards layout,
 * render and input; panels own their own widgets and state.
 */
public abstract class Panel {
    protected int x;
    protected int y;
    protected int width;
    protected int height;
    protected OverlayScreen screen;

    public void attach(OverlayScreen screen) {
        this.screen = screen;
    }

    public void setBounds(int x, int y, int width, int height) {
        this.x = x;
        this.y = y;
        this.width = width;
        this.height = height;
    }

    /** (Re)create widgets after bounds change. */
    public void init() {}

    public abstract void render(GuiGraphics graphics, int mouseX, int mouseY, float partialTick);

    public void tick() {}

    public boolean mouseClicked(double mouseX, double mouseY, int button) {
        return false;
    }

    public boolean mouseReleased(double mouseX, double mouseY, int button) {
        return false;
    }

    public boolean mouseDragged(double mouseX, double mouseY, int button, double dragX, double dragY) {
        return false;
    }

    public boolean mouseScrolled(double mouseX, double mouseY, double scrollX, double scrollY) {
        return false;
    }

    public boolean keyPressed(int keyCode, int scanCode, int modifiers) {
        return false;
    }

    public boolean charTyped(char codePoint, int modifiers) {
        return false;
    }

    /** Page hidden (switched away or screen closed). */
    public void onHide() {}
}
