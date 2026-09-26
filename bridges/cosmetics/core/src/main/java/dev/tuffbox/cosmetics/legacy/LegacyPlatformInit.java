package dev.tuffbox.cosmetics.legacy;

import dev.tuffbox.cosmetics.core.CosmeticsCore;

import java.lang.reflect.Method;
import java.nio.file.Path;
import java.nio.file.Paths;

/**
 * Best-effort Forge/Fabric bootstrap via reflection so scaffold jars can load
 * CosmeticsCore on legacy/mid anchors without compile-time MC deps.
 *
 * Full Tessellator / feature-renderer FX is ported per-anchor (see MATRIX.md).
 */
public final class LegacyPlatformInit {
    private LegacyPlatformInit() {}

    public static CosmeticsCore.Session init() {
        Path game = resolveGameDir();
        CosmeticsCore.Session session = CosmeticsCore.loadSession(game);
        if (session != null && session.username != null) {
            CosmeticsCore.put(session.username, CosmeticsCore.snapshotFromSession(session));
        }
        tryHookForgeBus(session);
        return session;
    }

    private static Path resolveGameDir() {
        try {
            Class<?> fml = Class.forName("net.minecraftforge.fml.loading.FMLPaths");
            Object gamedir = fml.getField("GAMEDIR").get(null);
            Method get = gamedir.getClass().getMethod("get");
            Object p = get.invoke(gamedir);
            if (p instanceof Path) {
                return (Path) p;
            }
        } catch (Throwable ignored) {
        }
        try {
            Class<?> fl = Class.forName("net.fabricmc.loader.api.FabricLoader");
            Object inst = fl.getMethod("getInstance").invoke(null);
            Object p = fl.getMethod("getGameDir").invoke(inst);
            if (p instanceof Path) {
                return (Path) p;
            }
        } catch (Throwable ignored) {
        }
        return Paths.get(".").toAbsolutePath().normalize();
    }

    /** Registers a no-op style listener if Forge EVENT_BUS is present (1.12–1.20). */
    private static void tryHookForgeBus(CosmeticsCore.Session session) {
        try {
            Class<?> forge = Class.forName("net.minecraftforge.common.MinecraftForge");
            Object bus = forge.getField("EVENT_BUS").get(null);
            // Presence check only — typed @SubscribeEvent hooks need MC at compile time.
            if (bus != null && session != null) {
                System.out.println("[tuffbox_cosmetics] Forge bus present; session=" + session.username);
            }
        } catch (Throwable ignored) {
        }
    }
}
