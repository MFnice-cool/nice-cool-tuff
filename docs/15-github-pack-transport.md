# 15. GitHub Pack Transport

Repo-native distribution of TuffBox packs through **public GitHub repositories**. Isolated from TuffSwarm (`docs/13-tuffswarm-network.md`). Friends install and update **anonymously** — no Microsoft, Modrinth, or GitHub account. Only the **author** authenticates (device OAuth or a PAT fallback).

This increment does **not** include private repos, PR/fork flow, `tuffbox://` deep links, server AutoModpack sync, ads, or antivirus scanning.

## Repository contract

A published pack is a Git tree, not a zip-only GitHub Release:

| Path | Role |
|------|------|
| `{id}.tuffbox.json` | Canonical TuffBox manifest (source of truth) |
| `{id}.tuffbox.lock.json` | Lockfile beside the manifest |
| `pack.toml` / `index.toml` / `mods/*.pw.toml` | packwiz interop (MC + loader live in `[versions]`) |
| overrides (`config/`, `kubejs/`, …) | Copied when present |
| `README.md` | Share instructions |
| `.tuffbox/repo-transport.json` | Transport metadata (`ready` / `publishing`, `managedFiles`, digest, signature) |

There is no separate `modpack.json` or `options.toml`. Custom jars under 50 MiB stay in git; larger jars are GitHub Release assets with a two-phase `publishing` → `ready` marker. Consumers ignore non-`ready` revisions.

### Transport schema v2

`TRANSPORT_SCHEMA_VERSION = 2`. Each `releaseAssets[]` entry must include `relativePath`, `sha512`, and `size`. Incomplete/legacy assets are rejected. Paths in the transport envelope are validated (no `..`, absolute, or drive prefixes). `releaseTag` and git refs use the same allowlist as source refs (`A–Z a–z 0–9 . _ - /`, no `..`).

Ed25519 signs a length-prefixed envelope (`tuffbox-transport-v2`) over the unsigned transport JSON plus the canonical manifest bytes — not the sidecar file alone.

Local install state is **never** published. It lives at `.tuffbox/transport.json` on the player machine (repo, ref, commit, version, pinned signer).

## Publish

1. Stage a deterministic tree (`stage_repo_tree`), including release-asset metadata and unique asset names (`{modId}-{fileName}`).
2. Sign the transport envelope + manifest with a local Ed25519 key (OS keyring).
3. Git Data API: blobs → tree (`base_tree` preserves foreign files) → one commit → fast-forward ref. `managedFiles` lets TuffBox delete only files it previously generated.
4. If large jars exist, upload Release assets after the commit, then flip status to `ready`.
5. Identical `contentDigest` is a no-op. Non-fast-forward updates are reported as conflicts; TuffBox never force-pushes.

Author login uses GitHub device flow (`public_repo` scope). Only the public OAuth client ID is shipped (`TUFFBOX_GITHUB_CLIENT_ID`). Progress phases: `staging` → `commit` → (`assets` when two-phase) → `done`.

## Consume

1. Parse `owner/repo`, `gh:owner/repo:tag`, or `https://github.com/owner/repo` (refs validated).
2. Inspect the public repo (name, version, `ready` / `publishing`) before install. Revisions still marked `publishing` are refused.
3. Download the commit tarball anonymously (host allowlist, 512 MiB stream cap, 90 s timeout), extract with path-traversal limits and symlink/hardlink rejection, prefer the sidecar manifest (packwiz is the fallback).
4. Validate transport metadata, verify content digest, verify signature over the v2 envelope when present.
5. Materialize Release assets to declared `relativePath` after size + SHA-512 checks (atomic `.tuffbox-part` → rename).
6. TOFU-pin the author public key; reject silent signer changes. A pinned signer **cannot** downgrade to an unsigned update.
7. Install is stage-then-promote into an empty target (non-empty destinations are refused). Project ids are slugified.
8. Updates: semantic diff → required review → snapshot with `managed_files` → apply the **reviewed commit SHA** → SHA-1/SHA-512 verify → rollback the snapshot on failure (rollback errors are surfaced).

Minecraft or loader changes require a full rematerialize (`requiresFullReinstall`): obsolete provider jars from the previous pack are removed, then missing files are downloaded through `mod_files`. Missing Local/GitHub hashed files are hard errors.

## Desktop API

`api.transport.github` in `apps/tuffbox-desktop/src/lib/api.ts`: parse, inspect, device login, stage preview, publish, install, check/preview/apply update (`applyUpdate(expectedCommitSha, path?)`). Library import previews the public repo then installs anonymously; Release Room publishes the pack tree with progress phases and conflict handling; an IDE banner requires diff review, custom-file / full-reinstall confirms, blocks signer changes, and shows the rollback snapshot id.

## Trust

- Modrinth/CurseForge files: known-origin CDN, hashes in the manifest.
- Custom GitHub jars/assets: untrusted custom content, shown in the update preview.
- Ed25519 sidecar signature is optional for unsigned first installs; when present it is verified on import. Once pinned, updates must keep the same signer.

## Patterns (not GPL code)

Prism: stage → validate → atomic apply. Modrinth Shared Instances: semantic diff → confirm → snapshot → incremental apply. TuffBox implements those flows against `{id}.tuffbox.json`, not against Prism/Modrinth sources.

## Acceptance

Automated coverage (must stay green):

- `cargo test -p tuffbox-core github_pack`
- `cargo test -p tuffbox-core --test github_pack_transport_red --test github_pack_transport_api_red`
- `cargo test -p tuffbox-desktop github_pack_commands::tests`
- Desktop frontend: `npm run check`, `npm test`, `npm run lint:perf`, `npm run build`

Live public-GitHub E2E (manual when `gh`/PAT available): publish v1 → anonymous install → noop republish → publish v2 with large jar → preview/apply → rollback; deny publishing revisions, signer change, tamper, and non-fast-forward.
