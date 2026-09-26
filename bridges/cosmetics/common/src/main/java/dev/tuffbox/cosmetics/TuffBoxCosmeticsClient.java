package dev.tuffbox.cosmetics;

import com.mojang.blaze3d.platform.InputConstants;
import dev.tuffbox.cosmetics.core.CosmeticsCore;
import net.fabricmc.api.ClientModInitializer;
import net.fabricmc.fabric.api.client.event.lifecycle.v1.ClientTickEvents;
import net.fabricmc.fabric.api.client.keybinding.v1.KeyBindingHelper;
import net.fabricmc.fabric.api.client.rendering.v1.LivingEntityFeatureRendererRegistrationCallback;
import net.minecraft.client.KeyMapping;
import net.minecraft.client.Minecraft;
import net.minecraft.client.renderer.entity.player.PlayerRenderer;
import net.minecraft.world.entity.EntityType;
import net.minecraft.world.entity.player.Player;
import org.lwjgl.glfw.GLFW;
import org.slf4j.Logger;
import org.slf4j.LoggerFactory;

/**
 * Client-only cosmetics bridge (Fabric 1.21.1). Skins/capes via CustomSkinLoader;
 * wings / hats / jump circles / trails / combat FX among TuffBox peers.
 * Open GUI with Right Shift (configurable in Controls).
 */
public final class TuffBoxCosmeticsClient implements ClientModInitializer {
    public static final Logger LOG = LoggerFactory.getLogger("tuffbox_cosmetics");

    private static KeyMapping openMenuKey;

    private CosmeticsCore.Session session;
    private int tick;

    @Override
    public void onInitializeClient() {
        session = CosmeticsCore.loadSession(Minecraft.getInstance().gameDirectory.toPath());
        CosmeticsClientConfig.get();
        VisualFx.init();
        CombatFx.init();

        openMenuKey = KeyBindingHelper.registerKeyBinding(new KeyMapping(
                "key.tuffbox_cosmetics.menu",
                InputConstants.Type.KEYSYM,
                GLFW.GLFW_KEY_RIGHT_SHIFT,
                "key.categories.tuffbox_cosmetics"
        ));

        LivingEntityFeatureRendererRegistrationCallback.EVENT.register(
                (entityType, entityRenderer, registrationHelper, context) -> {
                    if (entityType == EntityType.PLAYER && entityRenderer instanceof PlayerRenderer playerRenderer) {
                        registrationHelper.register(new WingsFeatureRenderer<>(playerRenderer));
                        registrationHelper.register(new ChinaHatFeatureRenderer<>(playerRenderer));
                    }
                });
        ClientTickEvents.END_CLIENT_TICK.register(client -> {
            while (openMenuKey.consumeClick()) {
                if (client.screen == null) {
                    client.setScreen(new CosmeticsMenuScreen(null));
                } else if (client.screen instanceof CosmeticsMenuScreen) {
                    client.screen.onClose();
                }
            }
            VisualFx.tick(client);
            CombatFx.tick(client);
            onTick(client);
        });
        if (session == null) {
            LOG.info("No .tuffbox/cosmetics-session.json — cosmetics sync idle");
        } else {
            CosmeticsCore.put(session.username, CosmeticsCore.snapshotFromSession(session));
            CosmeticsProfiles.applyLocalConfigToSelf();
            LOG.info("TuffBox cosmetics session for {} api={}", session.username, session.apiBase);
        }
        LOG.info("Cosmetics GUI: Right Shift (or rebind under Controls → TuffBox Cosmetics)");
    }

    private void onTick(Minecraft client) {
        if (session == null || client.level == null || client.player == null) return;
        if ((++tick % 100) != 0) return;
        for (Player player : client.level.players()) {
            String name = player.getGameProfile().getName();
            if (name == null || name.isBlank()) continue;
            String key = name.toLowerCase();
            if (CosmeticsCore.BY_NAME.containsKey(key)) continue;
            CosmeticsCore.put(key, new CosmeticsCore.Snapshot());
            Thread.startVirtualThread(() -> {
                CosmeticsCore.Snapshot snap = CosmeticsCore.fetchProfile(session, name);
                CosmeticsCore.put(key, snap);
            });
        }
    }

    /** @deprecated use {@link CosmeticsCore.Session} / {@link CosmeticsCore#loadSession} */
    @Deprecated
    public static final class CosmeticsSession extends CosmeticsCore.Session {
        public static CosmeticsSession load() {
            CosmeticsCore.Session s = CosmeticsCore.loadSession(
                    Minecraft.getInstance().gameDirectory.toPath());
            if (s == null) return null;
            CosmeticsSession out = new CosmeticsSession();
            out.username = s.username;
            out.uuid = s.uuid;
            out.apiBase = s.apiBase;
            out.anonKey = s.anonKey;
            out.wings = s.wings;
            out.hat = s.hat;
            out.trail = s.trail;
            out.jumpCircles = s.jumpCircles;
            out.hitParticles = s.hitParticles;
            out.hitBubbles = s.hitBubbles;
            out.targetEsp = s.targetEsp;
            out.killEffect = s.killEffect;
            return out;
        }
    }
}
