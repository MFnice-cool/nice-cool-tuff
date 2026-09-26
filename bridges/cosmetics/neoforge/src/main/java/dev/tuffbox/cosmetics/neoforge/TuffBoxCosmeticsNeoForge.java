package dev.tuffbox.cosmetics.neoforge;

import dev.tuffbox.cosmetics.core.CosmeticsCore;
import net.neoforged.api.distmarker.Dist;
import net.neoforged.bus.api.IEventBus;
import net.neoforged.fml.common.Mod;
import net.neoforged.fml.loading.FMLEnvironment;
import net.neoforged.fml.loading.FMLPaths;
import org.slf4j.Logger;
import org.slf4j.LoggerFactory;

/**
 * NeoForge 1.21.1 entry: loads shared CosmeticsCore session (incl. writeSecret).
 * Full FX/GUI port is tracked in PORT_NEOFORGE_1.21.1.md — keep separate from TuffSwarm 16C.
 */
@Mod(value = "tuffbox_cosmetics", dist = Dist.CLIENT)
public final class TuffBoxCosmeticsNeoForge {
    private static final Logger LOG = LoggerFactory.getLogger("tuffbox_cosmetics");

    public TuffBoxCosmeticsNeoForge(IEventBus modBus) {
        if (FMLEnvironment.dist != Dist.CLIENT) return;
        try {
            CosmeticsCore.Session session = CosmeticsCore.loadSession(FMLPaths.GAMEDIR.get());
            if (session != null) {
                CosmeticsCore.put(session.username, CosmeticsCore.snapshotFromSession(session));
                LOG.info(
                    "TuffBox cosmetics session for {} (NeoForge 1.21.1; writeSecret={})",
                    session.username,
                    session.writeSecret != null && session.writeSecret.length() >= 16 ? "yes" : "no"
                );
            } else {
                LOG.info("TuffBox cosmetics NeoForge — no .tuffbox/cosmetics-session.json");
            }
        } catch (Exception e) {
            LOG.warn("cosmetics neo init: {}", e.toString());
        }
    }
}
