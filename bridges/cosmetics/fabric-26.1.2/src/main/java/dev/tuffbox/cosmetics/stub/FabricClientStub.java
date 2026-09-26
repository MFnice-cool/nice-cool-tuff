package dev.tuffbox.cosmetics.stub;

import net.fabricmc.api.ClientModInitializer;

public final class FabricClientStub implements ClientModInitializer {
    @Override
    public void onInitializeClient() {
        PlatformBootstrap.load();
    }
}
