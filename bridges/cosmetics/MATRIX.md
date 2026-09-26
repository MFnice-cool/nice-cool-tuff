# TuffBox Cosmetics — version matrix

Inject picks the **highest anchor ≤ MC version** for the active loader.
See `crates/tuffbox-core/src/cosmetics_runtime.rs` (`resolve_cosmetics_artifact`).

## Anchors × loaders

| Anchor | Forge | Fabric | NeoForge | FX status |
|--------|-------|--------|----------|-----------|
| 1.12.2 | scaffold | — | — | session/core stub |
| 1.16.5 | scaffold | scaffold | — | session/core stub |
| 1.20.1 | scaffold | scaffold | scaffold | session/core stub |
| 1.21.1 | — | **full FX** | **next port** (session + core) | Fabric = production; see `PORT_NEOFORGE_1.21.1.md` |
| 1.21.4 | — | scaffold | scaffold | port from 1.21.1 |
| 1.21.11 | — | scaffold | scaffold | port from 1.21.1 |
| 26.1.2 | — | scaffold | scaffold | port from 1.21.1 |
| 26.2 | — | scaffold | scaffold | port from 1.21.1 |

Quilt → Fabric artifact.

## Jar naming

`tuffbox-cosmetics-{anchor}-{loader}-0.1.0.jar` in `prebuilt/`.

## Modules

- `core/` — Java 8 protocol (session, HTTP, HSV)
- `common/` + `fabric/` / `neoforge/` — 1.21.1 production
- `fabric-*` / `forge-*` / `neoforge-*` — scaffolds (`PlatformBootstrap` + core)

## Build

```bat
set JAVA_HOME=C:\Path\To\jdk-21
gradlew :core:jar :fabric:build syncDesktopResources
```

Legacy 1.12.2 toolchain may need JDK 8 for that module only.
