package dev.tuffbox.cosmetics.legacy;

/**
 * Port checklist for Forge 1.12.2 FX (Tessellator / GlStateManager).
 * Compile against Forge MDK when promoting this scaffold to full FX.
 *
 * <pre>
 * - Subscribe RenderWorldLastEvent → jump circles / trails / combat world FX
 * - Subscribe RenderPlayerEvent.Post → china hat / halo / horns / crown / wings
 * - AttackEntityEvent (client) → hit particles / bubbles / kill detect
 * - Reuse CosmeticsCore.hsva + session/profile cache
 * </pre>
 */
public final class Forge112PortNotes {
    private Forge112PortNotes() {}

    public static final String ANCHOR = "1.12.2";
    public static final String LOADER = "forge";
}
