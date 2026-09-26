package dev.tuffbox.cosmetics.stub;

import dev.tuffbox.cosmetics.core.CosmeticsCore;
import dev.tuffbox.cosmetics.legacy.LegacyPlatformInit;

/**
 * Session bootstrap for anchor scaffolds. Full FX ports replace this entry
 * with version-specific render hooks (see bridges/cosmetics/MATRIX.md).
 */
public final class PlatformBootstrap {
    private PlatformBootstrap() {}

    public static CosmeticsCore.Session load() {
        return LegacyPlatformInit.init();
    }

    public static void main(String[] args) {
        CosmeticsCore.Session s = load();
        System.out.println(s == null ? "no-session" : ("session:" + s.username));
    }
}
