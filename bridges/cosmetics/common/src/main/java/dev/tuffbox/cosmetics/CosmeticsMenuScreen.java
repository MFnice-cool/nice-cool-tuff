package dev.tuffbox.cosmetics;

import net.minecraft.client.gui.GuiGraphics;
import net.minecraft.client.gui.components.Button;
import net.minecraft.client.gui.components.CycleButton;
import net.minecraft.client.gui.screens.Screen;
import net.minecraft.network.chat.Component;

import java.util.List;

/**
 * In-game cosmetics control panel. Open with the TuffBox Cosmetics keybind (Right Shift).
 */
public final class CosmeticsMenuScreen extends Screen {
    private final Screen parent;
    private final CosmeticsClientConfig cfg;

    public CosmeticsMenuScreen(Screen parent) {
        super(Component.literal("TuffBox Cosmetics"));
        this.parent = parent;
        this.cfg = CosmeticsClientConfig.get();
    }

    @Override
    protected void init() {
        int cx = width / 2;
        int y = 36;
        int w = 160;
        int gap = 22;

        addRenderableWidget(CycleButton.onOffBuilder(cfg.master)
                .create(cx - w - 4, y, w, 20, Component.literal("Master"), (b, v) -> {
                    cfg.master = v;
                    persist();
                }));
        addRenderableWidget(CycleButton.onOffBuilder(cfg.showSelf)
                .create(cx + 4, y, w, 20, Component.literal("Show self"), (b, v) -> {
                    cfg.showSelf = v;
                    persist();
                }));
        y += gap;
        addRenderableWidget(CycleButton.onOffBuilder(cfg.showPeers)
                .create(cx - w - 4, y, w, 20, Component.literal("Show peers"), (b, v) -> {
                    cfg.showPeers = v;
                    persist();
                }));
        y += gap + 6;

        addRenderableWidget(CycleButton.onOffBuilder(cfg.wingsEnabled)
                .create(cx - w - 4, y, w, 20, Component.literal("Wings"), (b, v) -> {
                    cfg.wingsEnabled = v;
                    persist();
                }));
        int wingsIdx = CosmeticsClientConfig.indexOf(CosmeticsClientConfig.WINGS, cfg.wingsId);
        addRenderableWidget(CycleButton.<Integer>builder(
                        i -> Component.literal(CosmeticsClientConfig.WINGS_LABELS[i]))
                .withValues(List.of(0, 1, 2, 3))
                .withInitialValue(wingsIdx)
                .create(cx + 4, y, w, 20, Component.literal("Wings style"), (b, v) -> {
                    cfg.wingsId = CosmeticsClientConfig.WINGS[v];
                    cfg.wingsEnabled = !cfg.wingsId.isEmpty();
                    persist();
                }));
        y += gap;

        addRenderableWidget(CycleButton.onOffBuilder(cfg.hatEnabled)
                .create(cx - w - 4, y, w, 20, Component.literal("Hat"), (b, v) -> {
                    cfg.hatEnabled = v;
                    persist();
                }));
        int hatIdx = CosmeticsClientConfig.indexOf(CosmeticsClientConfig.HATS, cfg.hatId);
        addRenderableWidget(CycleButton.<Integer>builder(
                        i -> Component.literal(CosmeticsClientConfig.HATS_LABELS[i]))
                .withValues(List.of(0, 1, 2, 3, 4))
                .withInitialValue(hatIdx)
                .create(cx + 4, y, w, 20, Component.literal("Hat style"), (b, v) -> {
                    cfg.hatId = CosmeticsClientConfig.HATS[v];
                    cfg.hatEnabled = !cfg.hatId.isEmpty();
                    persist();
                }));
        y += gap + 6;

        addRenderableWidget(CycleButton.onOffBuilder(cfg.trail)
                .create(cx - w - 4, y, w, 20, Component.literal("Trail"), (b, v) -> {
                    cfg.trail = v;
                    persist();
                }));
        addRenderableWidget(CycleButton.onOffBuilder(cfg.jumpCircles)
                .create(cx + 4, y, w, 20, Component.literal("Jump circles"), (b, v) -> {
                    cfg.jumpCircles = v;
                    persist();
                }));
        y += gap;
        addRenderableWidget(CycleButton.onOffBuilder(cfg.hitParticles)
                .create(cx - w - 4, y, w, 20, Component.literal("Hit particles"), (b, v) -> {
                    cfg.hitParticles = v;
                    persist();
                }));
        addRenderableWidget(CycleButton.onOffBuilder(cfg.hitBubbles)
                .create(cx + 4, y, w, 20, Component.literal("Hit bubbles"), (b, v) -> {
                    cfg.hitBubbles = v;
                    persist();
                }));
        y += gap;
        addRenderableWidget(CycleButton.onOffBuilder(cfg.targetEsp)
                .create(cx - w - 4, y, w, 20, Component.literal("Target ESP"), (b, v) -> {
                    cfg.targetEsp = v;
                    persist();
                }));
        addRenderableWidget(CycleButton.onOffBuilder(cfg.killEffect)
                .create(cx + 4, y, w, 20, Component.literal("Kill effect"), (b, v) -> {
                    cfg.killEffect = v;
                    persist();
                }));
        y += gap + 12;

        addRenderableWidget(Button.builder(Component.literal("Reset to session"), b -> {
            CosmeticsClientConfig.reloadFromSession();
            rebuildWidgets();
        }).bounds(cx - w - 4, y, w, 20).build());
        addRenderableWidget(Button.builder(Component.literal("Done"), b -> onClose())
                .bounds(cx + 4, y, w, 20).build());
    }

    private void persist() {
        cfg.save();
        CosmeticsProfiles.applyLocalConfigToSelf();
    }

    @Override
    public void render(GuiGraphics g, int mouseX, int mouseY, float delta) {
        renderBackground(g, mouseX, mouseY, delta);
        super.render(g, mouseX, mouseY, delta);
        g.drawCenteredString(font, title, width / 2, 12, 0xFFFFFF);
        g.drawCenteredString(font, "Saved locally + shared to peers", width / 2, 24, 0xA0A0A0);
    }

    @Override
    public void onClose() {
        persist();
        if (minecraft != null) {
            minecraft.setScreen(parent);
        }
    }
}
