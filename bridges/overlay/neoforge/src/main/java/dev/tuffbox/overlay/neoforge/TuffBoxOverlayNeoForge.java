package dev.tuffbox.overlay.neoforge;

import dev.tuffbox.overlay.OverlayRuntime;
import dev.tuffbox.overlay.hud.PipHud;
import net.minecraft.client.KeyMapping;
import net.minecraft.client.Minecraft;
import net.minecraft.resources.ResourceLocation;
import net.neoforged.api.distmarker.Dist;
import net.neoforged.bus.api.IEventBus;
import net.neoforged.fml.common.Mod;
import net.neoforged.fml.loading.FMLEnvironment;
import net.neoforged.fml.loading.FMLPaths;
import net.neoforged.neoforge.client.event.ClientTickEvent;
import net.neoforged.neoforge.client.event.RegisterGuiLayersEvent;
import net.neoforged.neoforge.client.event.RegisterKeyMappingsEvent;
import net.neoforged.neoforge.common.NeoForge;

/**
 * NeoForge 1.21.1 entry: full overlay port — keybinds, tick loop and PiP HUD
 * delegate to the shared loader-neutral runtime (`common/` source tree).
 */
@Mod(value = "tuffbox_overlay", dist = Dist.CLIENT)
public final class TuffBoxOverlayNeoForge {

    public TuffBoxOverlayNeoForge(IEventBus modBus) {
        if (FMLEnvironment.dist != Dist.CLIENT) return;

        OverlayRuntime.bootstrap(FMLPaths.GAMEDIR.get());

        modBus.addListener(RegisterKeyMappingsEvent.class, event -> {
            KeyMapping open = OverlayRuntime.makeOpenKey();
            KeyMapping playPause = OverlayRuntime.makePlayPauseKey();
            KeyMapping stop = OverlayRuntime.makeStopKey();
            KeyMapping volUp = OverlayRuntime.makeVolumeUpKey();
            KeyMapping volDown = OverlayRuntime.makeVolumeDownKey();
            KeyMapping pip = OverlayRuntime.makeTogglePipKey();
            event.register(open);
            event.register(playPause);
            event.register(stop);
            event.register(volUp);
            event.register(volDown);
            event.register(pip);
            OverlayRuntime.bind(open, playPause, stop, volUp, volDown, pip);
        });

        modBus.addListener(RegisterGuiLayersEvent.class, event ->
                event.registerAboveAll(
                        ResourceLocation.fromNamespaceAndPath("tuffbox_overlay", "pip"),
                        PipHud::render));

        NeoForge.EVENT_BUS.addListener(ClientTickEvent.Post.class, event ->
                OverlayRuntime.tick(Minecraft.getInstance()));
    }
}
