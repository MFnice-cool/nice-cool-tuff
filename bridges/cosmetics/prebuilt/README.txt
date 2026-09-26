# TuffBox Cosmetics client bridge

## Build

Requires **JDK 21** for modern anchors (JDK 8 toolchain for `core` / `forge-1.12.2`).

```bat
cd bridges\cosmetics
set JAVA_HOME=C:\Path\To\jdk-21
gradlew syncDesktopResources
```

## Expected jar names (inject)

```
tuffbox-cosmetics-1.12.2-forge-*.jar
tuffbox-cosmetics-1.16.5-fabric-*.jar
tuffbox-cosmetics-1.16.5-forge-*.jar
tuffbox-cosmetics-1.20.1-fabric-*.jar
tuffbox-cosmetics-1.20.1-forge-*.jar
tuffbox-cosmetics-1.20.1-neoforge-*.jar
tuffbox-cosmetics-1.21.1-fabric-*.jar
tuffbox-cosmetics-1.21.1-neoforge-*.jar
tuffbox-cosmetics-1.21.4-fabric-*.jar
tuffbox-cosmetics-1.21.4-neoforge-*.jar
tuffbox-cosmetics-1.21.11-fabric-*.jar
tuffbox-cosmetics-1.21.11-neoforge-*.jar
tuffbox-cosmetics-26.1.2-fabric-*.jar
tuffbox-cosmetics-26.1.2-neoforge-*.jar
tuffbox-cosmetics-26.2-fabric-*.jar
tuffbox-cosmetics-26.2-neoforge-*.jar
```

Launcher resolves **highest anchor ≤ MC** for the loader. See `MATRIX.md` and `docs/14-cosmetics.md`.

## Session

`.tuffbox/cosmetics-session.json` (written by desktop launch): wings, hat, trail, jumpCircles, hitParticles, hitBubbles, targetEsp, killEffect.
