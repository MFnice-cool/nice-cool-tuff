package dev.tuffbox.overlay.media;

import net.minecraft.client.Minecraft;
import org.slf4j.Logger;
import org.slf4j.LoggerFactory;

import java.lang.reflect.Method;
import java.util.concurrent.Executor;

/**
 * Creates the media backend. WATERMeDIA classes (and loader mod-lists) are
 * touched only through reflection so a missing watermedia jar — or a
 * different loader — never breaks the overlay itself.
 */
public final class MediaBackends {
    private static final Logger LOG = LoggerFactory.getLogger("tuffbox_overlay");
    private static final String IMPL = "dev.tuffbox.overlay.media.watermedia.WaterMediaBackend";

    private MediaBackends() {}

    public static boolean watermediaInstalled() {
        if (fabricSaysLoaded()) {
            return true;
        }
        return neoforgeSaysLoaded();
    }

    private static boolean fabricSaysLoaded() {
        try {
            Class<?> cls = Class.forName("net.fabricmc.loader.api.FabricLoader");
            Object instance = cls.getMethod("getInstance").invoke(null);
            Method isModLoaded = cls.getMethod("isModLoaded", String.class);
            return Boolean.TRUE.equals(isModLoaded.invoke(instance, "watermedia"));
        } catch (Throwable ignored) {
            return false;
        }
    }

    private static boolean neoforgeSaysLoaded() {
        try {
            Class<?> cls = Class.forName("net.neoforged.fml.ModList");
            Object instance = cls.getMethod("get").invoke(null);
            Method isLoaded = cls.getMethod("isLoaded", String.class);
            return Boolean.TRUE.equals(isLoaded.invoke(instance, "watermedia"));
        } catch (Throwable ignored) {
            return false;
        }
    }

    /** New backend or null when WATERMeDIA is absent / failed to link. */
    public static MediaBackend create() {
        if (!watermediaInstalled()) {
            return null;
        }
        try {
            Class<?> cls = Class.forName(IMPL);
            return (MediaBackend) cls
                    .getConstructor(Executor.class)
                    .newInstance((Executor) Minecraft.getInstance());
        } catch (Throwable t) {
            LOG.warn("WATERMeDIA backend unavailable: {}", t.toString());
            return null;
        }
    }
}
