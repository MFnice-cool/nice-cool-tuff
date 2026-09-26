package dev.tuffbox.cosmetics;

import dev.tuffbox.cosmetics.core.CosmeticsCore;
import net.minecraft.client.Minecraft;
import net.minecraft.world.entity.player.Player;

/**
 * Thin adapter over {@link CosmeticsCore} + local {@link CosmeticsClientConfig} for rendering.
 */
public final class CosmeticsProfiles {
    private CosmeticsProfiles() {}

    public static CosmeticsCore.Snapshot get(String username) {
        return CosmeticsCore.get(username);
    }

    public static void put(String username, CosmeticsCore.Snapshot snap) {
        CosmeticsCore.put(username, snap);
    }

    /** @deprecated use {@link CosmeticsCore.Snapshot} */
    @Deprecated
    public static final class Snapshot extends CosmeticsCore.Snapshot {}

    public static String wingsFor(Player player) {
        if (!gate(player, true)) return "";
        CosmeticsClientConfig cfg = CosmeticsClientConfig.get();
        if (!cfg.wingsEnabled) return "";
        if (isSelf(player) && cfg.wingsId != null) {
            return cfg.wingsId;
        }
        CosmeticsCore.Snapshot s = fromPlayer(player);
        return s == null ? "" : s.wings;
    }

    public static String hatFor(Player player) {
        if (!gate(player, true)) return "";
        CosmeticsClientConfig cfg = CosmeticsClientConfig.get();
        if (!cfg.hatEnabled) return "";
        if (isSelf(player) && cfg.hatId != null) {
            return cfg.hatId;
        }
        CosmeticsCore.Snapshot s = fromPlayer(player);
        return s == null ? "" : s.hat;
    }

    public static boolean trailFor(Player player) {
        return flag(player, CosmeticsClientConfig.get().trail, s -> s.trail);
    }

    public static boolean jumpCirclesFor(Player player) {
        return flag(player, CosmeticsClientConfig.get().jumpCircles, s -> s.jumpCircles);
    }

    public static boolean hitParticlesFor(Player player) {
        return flag(player, CosmeticsClientConfig.get().hitParticles, s -> s.hitParticles);
    }

    public static boolean hitBubblesFor(Player player) {
        return flag(player, CosmeticsClientConfig.get().hitBubbles, s -> s.hitBubbles);
    }

    public static boolean targetEspFor(Player player) {
        return flag(player, CosmeticsClientConfig.get().targetEsp, s -> s.targetEsp);
    }

    public static boolean killEffectFor(Player player) {
        return flag(player, CosmeticsClientConfig.get().killEffect, s -> s.killEffect);
    }

    /** Push local GUI hat/wings/FX into the self cache entry. */
    public static void applyLocalConfigToSelf() {
        Minecraft mc = Minecraft.getInstance();
        if (mc.player == null) return;
        String name = mc.player.getGameProfile().getName();
        CosmeticsCore.Snapshot base = CosmeticsCore.get(name);
        if (base == null) {
            CosmeticsCore.Session sess = CosmeticsCore.loadSession(mc.gameDirectory.toPath());
            base = sess != null ? CosmeticsCore.snapshotFromSession(sess) : new CosmeticsCore.Snapshot();
        }
        CosmeticsClientConfig cfg = CosmeticsClientConfig.get();
        CosmeticsCore.Snapshot snap = new CosmeticsCore.Snapshot();
        snap.wings = cfg.wingsEnabled ? (cfg.wingsId == null ? "" : cfg.wingsId) : "";
        snap.hat = cfg.hatEnabled ? (cfg.hatId == null ? "" : cfg.hatId) : "";
        snap.trail = cfg.trail;
        snap.jumpCircles = cfg.jumpCircles;
        snap.hitParticles = cfg.hitParticles;
        snap.hitBubbles = cfg.hitBubbles;
        snap.targetEsp = cfg.targetEsp;
        snap.killEffect = cfg.killEffect;
        // Keep remote values if GUI ids empty but toggles on
        if (snap.wings.isEmpty() && cfg.wingsEnabled && base.wings != null) {
            snap.wings = base.wings;
        }
        if (snap.hat.isEmpty() && cfg.hatEnabled && base.hat != null) {
            snap.hat = base.hat;
        }
        CosmeticsCore.put(name, snap);
    }

    private interface Flag {
        boolean get(CosmeticsCore.Snapshot s);
    }

    private static boolean flag(Player player, boolean guiOn, Flag remote) {
        if (!gate(player, false)) return false;
        if (!guiOn) return false;
        if (isSelf(player)) return true;
        CosmeticsCore.Snapshot s = fromPlayer(player);
        return s != null && remote.get(s);
    }

    private static boolean gate(Player player, boolean accessory) {
        CosmeticsClientConfig cfg = CosmeticsClientConfig.get();
        if (!cfg.master) return false;
        if (isSelf(player)) return cfg.showSelf;
        return cfg.showPeers;
    }

    private static boolean isSelf(Player player) {
        Minecraft mc = Minecraft.getInstance();
        return mc.player != null && player != null && mc.player.getUUID().equals(player.getUUID());
    }

    private static CosmeticsCore.Snapshot fromPlayer(Player player) {
        if (player == null) return null;
        String name = player.getGameProfile().getName();
        CosmeticsCore.Snapshot s = CosmeticsCore.get(name);
        if (s != null) return s;
        try {
            CosmeticsCore.Session sess = CosmeticsCore.loadSession(
                    Minecraft.getInstance().gameDirectory.toPath());
            if (sess == null || name == null) return null;
            if (!name.equalsIgnoreCase(sess.username)) return null;
            return CosmeticsCore.snapshotFromSession(sess);
        } catch (Exception e) {
            return null;
        }
    }
}
