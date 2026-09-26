# 14. Competitive notes: GDLauncher Carbon

Date: 2026-07-22.  
Source reviewed: local tree `GDLauncher-Carbon-develop` (BSL 1.1 — **patterns only**, no code copy into production).

## Positioning

| | GDLauncher Carbon | TuffBox |
|--|-------------------|--------|
| Shell | Electron (Overwolf) + SolidJS | Tauri 2 + Svelte |
| Core | Separate `carbon_app` + rspc WebSocket | In-process Tauri commands |
| Focus | Play: instances, CF/Modrinth, Java, MS auth | Create + heal: ActionPlan, swarm, KubeJS/recipes |
| Status | Alpha (Discord); API key for local dev | Shipping product surface |

Users praise Carbon for **polished play UX**, not AI/IDE. TuffBox stays differentiated on **crash→fix flywheel, TuffSwarm, content authoring**.

## Steal list (adopted / planned)

| Idea | Status in TuffBox |
|------|-------------------|
| Resumable downloads + checksum + concurrency + stall timeout | Hardened in `tuffbox-core` `http` (this pass) |
| VisualTask-like progress bus | `task_progress` + desktop panel (this pass) |
| Potato / reduced-motion mode | Settings + CSS gate |
| Theme catalog + hover preview | Appearance tab (Carbon / Inferno / Aether / Frost / Pixelato / Win95 + TuffBox default) |
| Java / custom commands / runtime path / concurrent downloads / resolution / Discord RPC | Settings tabs + `launcher_settings` (this pass) |
| DB recovery ladder | Future |
| Honest importer capability matrix | Future |
| Bounded game log search | Partially in Diagnostics |

## Explicit non-goals

- Do **not** rewrite to Electron/Overwolf or rspc.
- Do **not** port VisualTask or `carbon_net` source verbatim (BSL).
- Do **not** chase CF partnership / ads surface.
- Creation Marketplace / tensors remain on TuffSwarm roadmap ([`13-tuffswarm-network.md`](13-tuffswarm-network.md)).

## Related

- OpenJar inspiration: [`11-openjar-inspiration-plan.md`](11-openjar-inspiration-plan.md)
- Architecture: [`03-architecture.md`](03-architecture.md)
