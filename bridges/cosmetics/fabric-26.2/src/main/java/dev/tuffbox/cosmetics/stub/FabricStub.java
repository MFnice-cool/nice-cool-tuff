package dev.tuffbox.cosmetics.stub;

import net.fabricmc.api.ModInitializer;

/** Fabric scaffold entry — loads CosmeticsCore session. */
public final class FabricStub implements ModInitializer {
    @Override
    public void onInitialize() {
        PlatformBootstrap.load();
    }
}
