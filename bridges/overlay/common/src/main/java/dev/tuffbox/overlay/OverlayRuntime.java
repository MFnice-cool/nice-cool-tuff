package dev.tuffbox.overlay;

import com.mojang.blaze3d.platform.InputConstants;
import dev.tuffbox.overlay.core.OverlayCore;
import dev.tuffbox.overlay.media.MediaController;
import dev.tuffbox.overlay.social.SocialService;
import net.minecraft.client.KeyMapping;
import net.minecraft.client.Minecraft;
import org.lwjgl.glfw.GLFW;
import org.slf4j.Logger;
import org.slf4j.LoggerFactory;

/**
 * Loader-neutral overlay runtime: keybind state, per-tick logic, session
 * bootstrap. Fabric and NeoForge entrypoints construct the KeyMappings and
 * delegate here, so both loaders share one behavior implementation.
 */
public final class OverlayRuntime {
    public static final Logger LOG = LoggerFactory.getLogger("tuffbox_overlay");

    private static KeyMapping openKey;
    private static KeyMapping playPauseKey;
    private static KeyMapping stopKey;
    private static KeyMapping volumeUpKey;
    private static KeyMapping volumeDownKey;
    private static KeyMapping togglePipKey;

    private static boolean bootstrapped;

    private OverlayRuntime() {}

    public static KeyMapping makeOpenKey() {
        return new KeyMapping("key.tuffbox_overlay.open", InputConstants.Type.KEYSYM,
                GLFW.GLFW_KEY_F8, "key.categories.tuffbox_overlay");
    }

    public static KeyMapping makePlayPauseKey() {
        return new KeyMapping("key.tuffbox_overlay.play_pause", InputConstants.Type.KEYSYM,
                GLFW.GLFW_KEY_F9, "key.categories.tuffbox_overlay");
    }

    public static KeyMapping makeStopKey() {
        return new KeyMapping("key.tuffbox_overlay.stop", InputConstants.Type.KEYSYM,
                GLFW.GLFW_KEY_F10, "key.categories.tuffbox_overlay");
    }

    public static KeyMapping makeVolumeUpKey() {
        return new KeyMapping("key.tuffbox_overlay.volume_up", InputConstants.Type.KEYSYM,
                GLFW.GLFW_KEY_PAGE_UP, "key.categories.tuffbox_overlay");
    }

    public static KeyMapping makeVolumeDownKey() {
        return new KeyMapping("key.tuffbox_overlay.volume_down", InputConstants.Type.KEYSYM,
                GLFW.GLFW_KEY_PAGE_DOWN, "key.categories.tuffbox_overlay");
    }

    public static KeyMapping makeTogglePipKey() {
        return new KeyMapping("key.tuffbox_overlay.toggle_pip", InputConstants.Type.KEYSYM,
                GLFW.GLFW_KEY_F7, "key.categories.tuffbox_overlay");
    }

    /** Called by loader glue once keymappings are constructed. */
    public static void bind(KeyMapping open, KeyMapping playPause, KeyMapping stop,
                            KeyMapping volUp, KeyMapping volDown, KeyMapping pip) {
        openKey = open;
        playPauseKey = playPause;
        stopKey = stop;
        volumeUpKey = volUp;
        volumeDownKey = volDown;
        togglePipKey = pip;
    }

    /** Config + session bootstrap (idempotent). Called from client init. */
    public static void bootstrap(java.nio.file.Path gameDir) {
        if (bootstrapped) {
            return;
        }
        bootstrapped = true;
        OverlayConfig.get();
        OverlayState.get().session = OverlayCore.loadSession(gameDir);
        if (OverlayState.get().session == null) {
            LOG.info("No .tuffbox/overlay-session.json — overlay social features idle");
        } else {
            LOG.info("TuffBox overlay session for {}", OverlayState.get().session.username);
        }
        LOG.info("Overlay GUI: F8 (or rebind under Controls → TuffBox Overlay)");
    }

    /** END_CLIENT_TICK equivalent — safe to call with any client state. */
    public static void tick(Minecraft client) {
        OverlayConfig cfg = OverlayConfig.get();
        while (openKey != null && openKey.consumeClick()) {
            if (!cfg.master) {
                continue;
            }
            if (client.screen == null) {
                client.setScreen(new OverlayScreen(null));
            } else if (client.screen instanceof OverlayScreen) {
                client.screen.onClose();
            }
        }

        MediaController media = OverlayState.get().media;
        while (playPauseKey != null && playPauseKey.consumeClick()) {
            media.toggle();
        }
        while (stopKey != null && stopKey.consumeClick()) {
            media.stop();
        }
        while (volumeUpKey != null && volumeUpKey.consumeClick()) {
            media.volumeUp();
        }
        while (volumeDownKey != null && volumeDownKey.consumeClick()) {
            media.volumeDown();
        }
        while (togglePipKey != null && togglePipKey.consumeClick()) {
            cfg.pipEnabled = !cfg.pipEnabled;
            cfg.save();
        }

        media.tick();
        SocialService.get().tick();
    }

    /** Screen-level key match so the open key also closes the overlay. */
    public static boolean matchesOpenKey(int keyCode, int scanCode) {
        return openKey != null && openKey.matches(keyCode, scanCode);
    }
}
