package dev.tuffbox.overlay;

import dev.tuffbox.overlay.hud.PipHud;
import net.fabricmc.api.ClientModInitializer;
import net.fabricmc.fabric.api.client.event.lifecycle.v1.ClientTickEvents;
import net.fabricmc.fabric.api.client.keybinding.v1.KeyBindingHelper;
import net.fabricmc.fabric.api.client.rendering.v1.HudRenderCallback;
import net.minecraft.client.Minecraft;

/**
 * Fabric entrypoint: registers keybinds/HUD and delegates all behavior to
 * the loader-neutral OverlayRuntime.
 */
public final class TuffBoxOverlayClient implements ClientModInitializer {

    @Override
    public void onInitializeClient() {
        OverlayRuntime.bootstrap(Minecraft.getInstance().gameDirectory.toPath());
        OverlayRuntime.bind(
                KeyBindingHelper.registerKeyBinding(OverlayRuntime.makeOpenKey()),
                KeyBindingHelper.registerKeyBinding(OverlayRuntime.makePlayPauseKey()),
                KeyBindingHelper.registerKeyBinding(OverlayRuntime.makeStopKey()),
                KeyBindingHelper.registerKeyBinding(OverlayRuntime.makeVolumeUpKey()),
                KeyBindingHelper.registerKeyBinding(OverlayRuntime.makeVolumeDownKey()),
                KeyBindingHelper.registerKeyBinding(OverlayRuntime.makeTogglePipKey()));

        ClientTickEvents.END_CLIENT_TICK.register(OverlayRuntime::tick);
        HudRenderCallback.EVENT.register(PipHud::render);
    }
}
