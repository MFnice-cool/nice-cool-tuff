# Port checklist — NeoForge 1.21.1 (next FX anchor)

Priority after Fabric 1.21.1 full FX. Goal: ship the same GUI + renderers on NeoForge.

## Done / available now

- [x] `neoforge/` module loads `CosmeticsCore` session
- [x] Session includes `writeSecret`; in-game upsert lives in `CosmeticsCore` (loader-agnostic)
- [x] Fabric `common/` owns menu + FX (reference implementation)

## Remaining (do not mix with TuffSwarm 16C)

1. Add NeoForge client init that registers Right-Shift keybind + opens a NeoForge-mapped `CosmeticsMenuScreen` (or shared screen with NF event bus).
2. Port feature renderers from `common/` (`WingsFeatureRenderer`, hats, trails, combat FX) onto NeoForge player render layers.
3. Wire peer poll (`cosmetics-get`) on client tick like Fabric `TuffBoxCosmeticsClient`.
4. Build jar → `prebuilt/tuffbox-cosmetics-1.21.1-neoforge-0.1.0.jar` and `syncDesktopResources`.
5. Flip MATRIX.md NeoForge 1.21.1 from `session + core` → **full FX**.

## Out of scope here

- Fabric 1.20.1 (second priority after NeoForge 1.21.1)
- TuffSwarm Phase C P2P (`docs/13-tuffswarm-network.md`) — separate track
