package dev.tuffbox.jeibridge;

import mezz.jei.api.IModPlugin;
import mezz.jei.api.JeiPlugin;
import mezz.jei.api.runtime.IJeiRuntime;
import net.minecraft.resources.ResourceLocation;

@JeiPlugin
public final class TuffBoxJeiPlugin implements IModPlugin {
    private static final ResourceLocation UID =
        ResourceLocation.fromNamespaceAndPath("tuffbox", "runtime_bridge");

    @Override
    public ResourceLocation getPluginUid() {
        return UID;
    }

    @Override
    public void onRuntimeAvailable(IJeiRuntime jeiRuntime) {
        BridgeServer.start(jeiRuntime);
    }

    @Override
    public void onRuntimeUnavailable() {
        BridgeServer.stop();
    }
}
