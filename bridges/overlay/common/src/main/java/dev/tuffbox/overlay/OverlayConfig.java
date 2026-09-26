package dev.tuffbox.overlay;

import dev.tuffbox.overlay.core.OverlayCore;
import net.minecraft.client.Minecraft;

import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;

/**
 * In-game overlay preferences, persisted to .tuffbox/overlay-gui.json.
 * The launcher session file stays launcher-owned; this file is mod-owned.
 */
public final class OverlayConfig {
    private static OverlayConfig instance;

    public boolean master = true;
    public boolean pipEnabled = true;
    /** PiP corner anchor: 0=top-left, 1=top-right, 2=bottom-left, 3=bottom-right. */
    public int pipCorner = 1;
    /** PiP width scale preset: 0=small, 1=medium, 2=large. */
    public int pipScale = 1;
    /** Media volume 0..100. */
    public int volume = 50;
    /** Share presence (online/pack) with friends. */
    public boolean presenceOptIn = true;
    /** Last opened rail page: 0=YouTube, 1=Friends, 2=Chat. */
    public int lastPage = 0;

    private OverlayConfig() {}

    public static OverlayConfig get() {
        if (instance == null) {
            instance = load();
        }
        return instance;
    }

    private static Path path() {
        return Minecraft.getInstance().gameDirectory.toPath()
                .resolve(".tuffbox").resolve("overlay-gui.json");
    }

    private static OverlayConfig load() {
        OverlayConfig cfg = new OverlayConfig();
        try {
            Path p = path();
            if (Files.isRegularFile(p)) {
                String json = new String(Files.readAllBytes(p), StandardCharsets.UTF_8);
                cfg.master = OverlayCore.jsonBool(json, "master", true);
                cfg.pipEnabled = OverlayCore.jsonBool(json, "pipEnabled", true);
                cfg.presenceOptIn = OverlayCore.jsonBool(json, "presenceOptIn", true);
                cfg.pipCorner = (int) OverlayCore.jsonLong(json, "pipCorner", 1);
                cfg.pipScale = (int) OverlayCore.jsonLong(json, "pipScale", 1);
                cfg.volume = (int) OverlayCore.jsonLong(json, "volume", 50);
                cfg.lastPage = (int) OverlayCore.jsonLong(json, "lastPage", 0);
            }
        } catch (Exception ignored) {
        }
        return cfg;
    }

    public void save() {
        try {
            Path p = path();
            Files.createDirectories(p.getParent());
            String json = "{\n"
                    + "  \"master\": " + master + ",\n"
                    + "  \"pipEnabled\": " + pipEnabled + ",\n"
                    + "  \"presenceOptIn\": " + presenceOptIn + ",\n"
                    + "  \"pipCorner\": " + pipCorner + ",\n"
                    + "  \"pipScale\": " + pipScale + ",\n"
                    + "  \"volume\": " + volume + ",\n"
                    + "  \"lastPage\": " + lastPage + "\n"
                    + "}\n";
            Files.write(p, json.getBytes(StandardCharsets.UTF_8));
        } catch (Exception ignored) {
        }
    }
}
