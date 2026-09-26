# JEI Runtime Bridge

TuffBox Recipes has two data sources:

1. **Live JEI** for a running Minecraft 1.21.1 client on Fabric or NeoForge.
2. **Offline files** from mod JARs, datapacks and KubeJS when the client is not running.

## Lifecycle

On launch, TuffBox checks that the project uses a supported loader/version and
contains JEI. It temporarily copies the matching bridge jar into `mods/`, adds
two JVM properties (a random token and handshake path), and removes the bridge
jar and handshake after the game exits. The bridge is never added to the
project manifest or exported modpack.

JEI calls `TuffBoxJeiPlugin.onRuntimeAvailable` after its runtime registry is
ready. The plugin binds an ephemeral port on `127.0.0.1` and writes
`.tuffbox/jei-bridge.json`. TuffBox reads that file, but the token is kept in
Rust and is never exposed to Svelte.

## Protocol v1

Every request requires `X-TuffBox-Token`.

- `GET /health` and `GET /v1/meta`
- `GET /v1/snapshot`
- `GET /v1/categories`
- `GET /v1/recipes`

The snapshot contains JEI recipe categories, crafting stations, normalized
recipe slot rectangles, ingredient alternatives, localized display names and
tooltip text. Calls touching JEI are executed on Minecraft's client thread.
Raw mod recipe objects are never serialized.

## Version matrix

- Fabric 1.21.1 + JEI 19.21: live bridge.
- NeoForge 1.21.1 + JEI 19.21: live bridge.
- Other versions/loaders, missing JEI, or stopped client: offline fallback.

Each Minecraft/JEI line needs its own bridge build because JEI API and Minecraft
mappings change between versions.

## Build

From `bridges/jei-runtime`:

```text
gradlew build
gradlew syncDesktopResources
```

The desktop bundle includes the Fabric and NeoForge jars from each module's
`build/libs` directory. During development, set `TUFFBOX_JEI_BRIDGE_DIR` to a
directory containing the two jars if they are not in their normal build paths.

## Security and limits

- Loopback only; non-loopback handshakes are rejected.
- Read-only API; no recipe transfer or game mutation endpoints.
- 8,000-recipe snapshot limit with a `truncated` marker.
- 35-second desktop timeout and 30-second client-thread timeout.
- Custom JEI drawables cannot always be converted to portable images; slot
  data and ingredients remain available when a custom background cannot be
  rendered.
