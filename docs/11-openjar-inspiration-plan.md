# 11. План: заимствования из OpenJar Launcher

Анализ OpenJar Launcher (macOS-first, Tauri v1 + React 18, ~20K строк Rust backend).
Цель — заимстовать проверенные паттерны, которые усиливают TuffBox без дублирования.

Дата: 2026-07-16.

---

## Что уже есть в TuffBox (не копировать)

| Фича | OpenJar | TuffBox |
|-------|---------|---------|
| MS Device Code OAuth | да | да |
| XBL → XSTS → MC auth chain | да | да |
| Keychain token storage | да | да |
| Offline login + MD5 UUID | нет | да |
| Modrinth provider | нет | да |
| Snapshot / rollback | нет | да |
| Crash parser | базовый | Diagnose 2.0 |
| Config editor | нет | да |
| Graph visualization | нет | да |
| Export .mrpack / Prism / CF | нет | да |
| Export server pack | нет | да |
| Forge / NeoForge / Fabric / Quilt install | да | да |

---

## Приоритет 1 — Скин-система (2–3 дня)

OpenJar имеет полноценную скин-систему. TuffBox пока делает только 2D avatar.

### 1.1 3D Skin Preview (skinview3d)

**Что берём:** npm-пакет `skinview3d` v3.4.1 (Three.js-based).

**Реализация:**
- Новый компонент `SkinPreview3D.svelte`
- Canvas-based: создать offscreen canvas → `new SkinViewer({ canvas })` → `viewer.loadSkin(url)`
- Thumbnail-режим: статичный кадр → `canvas.toDataURL()` → кешировать в store
- Interactive-режим: drag-to-rotate, zoom, damping (`minDistance: 24, maxDistance: 88`)
- Emotes: idle, wave, nod, celebrate (из `skinview3d` animation API)

**Файлы:**
- `apps/tuffbox-desktop/src/components/SkinPreview3D.svelte` — новый
- `apps/tuffbox-desktop/src/lib/skin-cache.ts` — кеш thumbnails (Map<uuid, dataUrl>, max 240)

**Зависимость:**
```bash
npm install skinview3d
```

### 1.2 Skin Management

**Что берём:** Saved skins library + custom skin import + default skins.

**Реализация:**
- `mc_saved_skins.json` в config dir: `{ id, label, url, variant, localPath? }[]`
- Default skins: Steve (classic), Alex (slim) из `mc-heads.net`
- Custom import: file picker → copy PNG в `skins/` dir → register в `mc_saved_skins.json`
- Rename / remove saved skins

**Tauri commands (добавить в `auth.rs` или новый `skins.rs`):**
```
mc_list_saved_skins     → Vec<SavedSkin>
mc_add_saved_skin       → SavedSkin (url or local path)
mc_rename_saved_skin    → ()
mc_remove_saved_skin    → ()
mc_get_skin_thumbnail   → dataUrl (cache miss → render → cache)
```

### 1.3 Skin Upload к Mojang

**Что берём:** POST скин через URL или multipart upload.

**Реализация:**
- `mc_apply_skin(skin_id: String, variant: String)` — POST `{ variant, url }` к `MC_PROFILE_URL/skins`
- File-based fallback: download URL → multipart upload
- `mc_apply_cape(cape_id: String)` — POST к `MC_PROFILE_URL/capes/{cape_id}/activate`

**Источник:** `main.rs:11428-11509` (OpenJar).

### 1.4 Cape Support

**Что берём:** Cape listing и cycling.

**Реализация:**
- Profile response уже содержит `capes: Vec<McProfileCosmetic>`
- `mc_list_capes()` → Vec<Cape> (id, alias, url, state)
- `mc_apply_cape(cape_id)` → activate

---

## Приоритет 2 — Multi-Account (1–2 дня)

### 2.1 Multi-account storage

**Что берём:** Per-account refresh tokens в keychain, account switching.

**Реализация:**
- Keyring key: `dev.tuffbox.ide.mc_account_{uuid}` (per-account refresh token)
- `mc_accounts.json`: `Vec<AccountEntry>` — `{ uuid, name, skinUrl, loginType, addedAt }`
- `mc_switch_account(uuid)` — load tokens, refresh, update active
- UI: account selector dropdown в Dashboard/Settings

**Tauri commands:**
```
mc_list_accounts        → Vec<AccountEntry>
mc_switch_account       → ()
mc_remove_account       → ()
```

### 2.2 MC Launcher Auth Endpoint

**Что берём:** `MC_LAUNCHER_AUTH_URL` (`api.minecraftservices.com/launcher/login`) — более надёжный endpoint для launcher tokens.

**Изменение в `auth.rs`:**
- Текущий fallback: `login_with_xbox`
- Добавить: сначала `launcher/login` с identity token, если 401 → fallback на `login_with_xbox`
- Добавить: `ensure_minecraft_entitlement()` — GET `mcstore` entitlements для проверки ownership

**Источник:** `main.rs:11296-11330`.

### 2.3 Entitlement Check

**Что берём:** Проверка владения Minecraft перед запуском.

**Реализация:**
- `mc_check_entitlement()` → GET `api.minecraftservices.com/entitlements/mcstore`
- Если нет `game_minecraft` → показать предупреждение, не блокировать (для оффлайна)

---

## Приоритет 3 — Lockfile-трекинг контента (2–3 дня)

### 3.1 Full lockfile с provider metadata

**Что берём:** Каждый установленный файл трекается с provider ID, version, hash, pin state.

**Реализация:**
- Расширить `TuffboxLockfile`:
  ```rust
  struct LockfileEntry {
      mod_id: String,
      source: ContentSource,  // Modrinth { project_id, file_id }, LocalJar, ElybySkin...
      version: String,
      file_hash: String,       // sha1
      file_size: u64,
      installed_at: String,    // RFC3339
      enabled: bool,
      pinned: bool,
  }
  ```
- `lock.json` хранится в `.tuffbox/lock.json`
- При каждом add/update/remove — обновлять lockfile
- Lockfile → source of truth для installed state (вместо сканирования `mods/`)

**Источник:** OpenJar `lock.json` формат.

### 3.2 Enable/Disable mods (без удаления)

**Что берём:** Toggle mods без физического удаления.

**Реализация:**
- Mods хранятся в `mods/disabled/` когда disabled
- Lockfile поле `enabled: bool`
- `mc_toggle_mod(mod_id, enabled)` → move файл + update lock

---

## Приоритет 4 — Multi-Provider Discovery (3–4 дня)

### 4.1 CurseForge Provider

**Что берём:** CurseForge API как первый провайдер после Modrinth.

**Реализация:**
- `CurseForgeProvider` implements `ContentProvider`
- API: `api.curseforge.com/v1`
- Search: `GET /v1/mods/search?gameId=432&classId=6&searchFilter={query}`
- Versions: `GET /v1/mods/{modId}/files`
- API key required: `x-api-key` header
- Settings: API key input в настройках

**Файлы:**
- `crates/tuffbox-core/src/providers/curseforge.rs` — новый
- Settings UI: CurseForge API key field

### 4.2 GitHub Releases Provider

**Что берём:** GitHub как источник модов (поисковая выдача, релизы).

**Реализация:**
- `GitHubProvider` implements `ContentProvider`
- Search: `GET /search/repositories?q={query}+minecraft+mod`
- Releases: `GET /repos/{owner}/{repo}/releases`
- Token pool: optional `github_token` в settings для rate limit (5000/hr → 50000/hr)
- Auto-detect: если jar содержит `fabric.mod.json` → fabric mod, `mods.toml` → forge mod

**Файлы:**
- `crates/tuffbox-core/src/providers/github.rs` — новый

### 4.3 Combined Search UI

**Что берём:** Unified search across all providers.

**Реализация:**
- `search_all_providers(query, mc_version, loader)` → `Vec<UnifiedSearchResult>`
- Each result tagged with `provider: "modrinth" | "curseforge" | "github"`
- Dedup by slug/name when same project exists on multiple providers
- UI: provider filter pills (All / Modrinth / CurseForge / GitHub)

---

## Приоритет 5 — Run Reports & Diagnostics (1–2 дня)

### 5.1 Post-launch Run Reports

**Что берём:** Structured run report после каждого запуска.

**Реализация:**
- `test-runs.json`: `Vec<RunReport>`
  ```rust
  struct RunReport {
      run_id: String,
      started_at: String,
      exited_at: Option<String>,
      exit_code: Option<i32>,
      duration_ms: Option<u64>,
      mc_version: String,
      loader: String,
      java_version: String,
      mods_count: u32,
      log_path: String,
      findings: Vec<RunFinding>,  // parsed from log
  }
  ```
- `RunFinding`: `{ level: "error"|"warn"|"info", source: "mod"|"loader"|"mc", message: String }`
- Parse `latest.log` + `crash-reports/*.txt` после завершения процесса

### 5.2 Run History UI

**Что берём:** Timeline-список запусков с expand для просмотра логов.

**Реализация:**
- Вкладка "History" в IDE workflow
- Показывает: время, длительность, exit code, mods count, warnings count
- Click → раскрывает findings + link к full log

---

## Приоритет 6 — Quick Play (0.5 дня)

### 6.1 Saved Server Targets

**Что берём:** Quick launch directly to a server.

**Реализация:**
- `quick-play-servers.json`: `Vec<ServerTarget>` — `{ name, address, port }`
- `mc_launch_with_quick_play(server)` → добавить `--quickPlayPath {servers_file}` в game args
- UI: Quick Play dropdown на Dashboard, рядом с Play

**Источник:** `main.rs:96-119`, `QUICK_PLAY_SERVERS_FILE`.

---

## Приоритет 7 — Storage Manager (1 день)

### 7.1 Disk Usage Analysis

**Что берём:** Показать где сидят гигабайты.

**Реализация:**
- Scan `mods/`, `resourcepacks/`, `shaderpacks/`, `logs/`, `crash-reports/`, `snapshots/`, `config/`
- Показать: total size per directory, top-10 largest files
- Cleanup actions: delete old logs, prune old snapshots, remove disabled mods

**Tauri commands:**
```
mc_get_storage_usage    → StorageReport { dirs: Vec<DirUsage>, largest_files: Vec<FileEntry> }
mc_cleanup_old_logs     → usize (bytes freed)
mc_prune_snapshots      → usize (bytes freed)
```

---

## Приоритет 8 — Creator Studio / Modpack Maker (2–3 дня)

### 8.1 Layered Pack Building

**Что берём:** Template + additions + overrides → final pack.

**Реализация:**
- Pack profile: `{ template: "fabric-1.20.1", additions: [...], overrides: { configs: {...} } }`
- UI: visual diff between template and current state
- "Pack Preview" → show final state before export
- Profiles: Lite / Recommended / Full (subset of mods)

### 8.2 Pack Distribution

**Что берём:** Quick share via link.

**Реализация:**
- `mc_export_pack_link()` → generate `.tuffbox.json` with all metadata
- Optional: upload to GitHub Gist (public, anonymous)

---

## Приоритет 9 — Scheduled Updates (1 день)

### 9.1 Update Check Cadence

**Что берём:** Hourly → weekly backoff for update checks.

**Реализация:**
- `last_update_check: String` в settings
- Algorithm: on app start, check if `now - last_check > interval`
  - Default: hourly. If no updates found 3 times → extend to 4h → 12h → 24h → 7d
- Compare installed versions vs available Modrinth/CurseForge versions
- UI: badge "Updates available" on sidebar

### 9.2 Snapshot Before Bulk Update

**Что берём:** Auto-snapshot before "Update All".

**Реализация:**
- `mc_update_all()` → create snapshot → update each mod → if any failure → offer rollback
- Show progress: "Updating 5/12 mods..."

---

## Приоритет 10 — Friend Link / P2P Sync (Future, 2+ дня)

### 10.1 P2P Pack Sync

**Что берём:** UPnP-based peer discovery for multiplayer groups.

**Реализация:**
- `igd` crate для UPnP port forwarding
- Broadcast pack hash on LAN → peers compare → sync differing mods
- Optional feature, opt-in в settings

**Примечание:** Это P2-P3 фича, отложить до стабильного core.

---

## Приоритет 11 — Minecraft Settings Sync (0.5 дня)

### 11.1 Options Sync

**Что берём:** Копировать `options.txt` / `servers.dat` между instances.

**Реализация:**
- `mc_sync_settings(from_instance, to_instance, what: "options"|"servers"|"all")`
- Atomic write: write to `.tmp` → rename

---

## Итого: Таблица приоритетов

| # | Фича | Дни | Сложность | Зависит от |
|---|------|-----|-----------|------------|
| P1 | 3D Skin Preview (skinview3d) | 1 | низкая | — |
| P1 | Skin Management (library + import) | 1 | средняя | 3D Preview |
| P1 | Skin Upload к Mojang | 0.5 | средняя | Auth |
| P1 | Cape Support | 0.5 | низкая | Auth |
| P2 | Multi-Account | 1.5 | средняя | — |
| P2 | MC Launcher Auth + Entitlement | 0.5 | низкая | — |
| P3 | Lockfile + Enable/Disable | 2 | средняя | — |
| P4 | CurseForge Provider | 1.5 | средняя | ContentProvider trait |
| P4 | GitHub Releases Provider | 1.5 | средняя | ContentProvider trait |
| P4 | Combined Search UI | 1 | низкая | Providers |
| P5 | Run Reports + History UI | 1.5 | средняя | Test Launcher |
| P6 | Quick Play | 0.5 | низкая | — |
| P7 | Storage Manager | 1 | низкая | — |
| P8 | Creator Studio | 2 | высокая | Manifest + Export |
| P9 | Scheduled Updates | 1 | средняя | Modrinth Provider |
| P10 | Friend Link (P2P) | 2+ | высокая | igd, UPnP |
| P11 | Settings Sync | 0.5 | низкая | — |

**Итого ~17 дней** для P1–P9.

---

## Рекомендуемый порядок

```
Неделя 1: P1 (Skin system) + P2 (Multi-account + auth)
Неделя 2: P3 (Lockfile) + P4 (CurseForge + GitHub)
Неделя 3: P5 (Run reports) + P6 (Quick Play) + P7 (Storage)
Неделя 4: P8 (Creator Studio) + P9 (Scheduled Updates)
Позже:   P10 (Friend Link) + P11 (Settings Sync)
```

---

## Технические детали заимствований

### API Endpoints (из OpenJar)

| Endpoint | Use |
|----------|-----|
| `api.minecraftservices.com/launcher/login` | More reliable MC launcher token |
| `api.minecraftservices.com/entitlements/mcstore` | Ownership check |
| `api.minecraftservices.com/minecraft/profile/skins` | Skin upload |
| `api.curseforge.com/v1/mods/search` | CurseForge search |
| `api.curseforge.com/v1/mods/{id}/files` | CurseForge versions |
| `api.github.com/search/repositories` | GitHub mod search |
| `api.github.com/repos/{o}/{r}/releases` | GitHub releases |

### OpenJar npm Dependencies to Adopt

| Package | Version | Use |
|---------|---------|-----|
| `skinview3d` | 3.4.1 | 3D skin viewer |

### OpenJar Rust Crates to Consider

| Crate | Version | Use |
|-------|---------|-----|
| `igd` | 0.12 | UPnP (P10 only) |
| `chacha20poly1305` | 0.10 | Encryption (if needed for local secrets) |

### OpenJar Patterns to Adopt

1. **Token refresh chain**: MSA → XBL → XSTS → MC Launcher (more reliable than login_with_xbox)
2. **Entitlement check**: Verify ownership before launch
3. **Lockfile as source of truth**: Don't scan mods/ folder, trust lock.json
4. **Enable/disable without delete**: Move to `mods/disabled/`
5. **Storage analysis**: Show disk usage per category
6. **Run reports**: Structured post-launch diagnostics
7. **Saved skins library**: Persistent skin collection across sessions

---

## Риски и ограничения

| Риск | Mitigation |
|------|------------|
| CurseForge requires API key | Settings UI + env var fallback |
| GitHub rate limits without token | Token pool + graceful degradation |
| skinview3d is Three.js (heavy) | Use for thumbnail only, lazy-load interactive |
| Multi-account keychain complexity | Start simple: one active + list of known |
| UPnP may be blocked on networks | Feature flag, fallback to manual |
