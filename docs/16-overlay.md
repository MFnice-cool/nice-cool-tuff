# TuffBox Overlay (universal GL hook)

Steam-style overlay for **any Minecraft version / any loader**: a native DLL is injected into
the game process and detours `opengl32!wglSwapBuffers`, drawing UI into the game framebuffer
(works in **exclusive fullscreen**).

## Architecture

Two processes, no Chromium inside the game:

| Process | Role |
|---------|------|
| **TuffBox launcher (Tauri)** | Backend / IPC proxy — Supabase friends/chat/presence, `youtube_feed`, session |
| **Game JVM + `tuffbox_overlay_hook.dll`** | Frontend — OpenGL present-hook, immediate UI, bitmap font, LRU GPU thumbnails, libmpv frames + PiP |

```
Play → write .tuffbox/overlay-session.json
     → start localhost IPC (TUFFBOX_OVERLAY_IPC)
     → spawn JVM with env
     → inject tuffbox_overlay_hook.dll
     → detour wglSwapBuffers → F8 UI
```

### IPC endpoints (`http://127.0.0.1:{port}`)

| Method | Path | Purpose |
|--------|------|---------|
| `GET` | `/health` | Liveness |
| `GET` | `/session` | Username / uuid / packName |
| `GET` | `/youtube-feed` | Normalized feed items |
| `GET` | `/youtube-resolve?id=` | Watch URL for libmpv |
| `GET`/`POST` | `/friends` | List friends + requests |
| `POST` | `/friends/action` | `{ action: add\|accept\|remove, … }` |
| `GET` | `/presence` | Heartbeat + friends online |
| `GET` | `/chat?sinceId=` | Incremental DM poll |
| `POST` | `/chat/send` | `{ toKey, body }` |

### Hook UI (F8)

Discord-dark shell matching the legacy JVM overlay:

- **Rail** — YouTube / Friends / Chat (unread badge on Chat)
- **Top bar** — page title, pack name, player name
- **YouTube**
  - Search/filter bar (live filter by title/channel/id)
  - Paste `youtube.com/watch?v=` / `youtu.be/` / bare id → Play URL
  - Feed list with LRU thumbnails + titles
  - In-framebuffer video via libmpv render API → offscreen FBO → blit
  - Seek bar, Pause/Stop, volume
  - PiP toggle / corner / size (survives closing F8)
- **Friends** — add-by-username, incoming accept/decline, online presence, Chat / Remove
- **Chat** — conversation list, message history, text input + Send, unread
- **Input**
  - LL-keyboard hook swallows game keys while open; printable chars → focused fields
  - LL-mouse hook captures **wheel** (↑/↓/PgUp/PgDn still work as fallback)
  - Esc / F8 closes
- **Global media keys** (overlay open or closed): **F9** pause/resume, **F10** stop
- **PiP HUD** — when overlay is closed and media is active, a corner widget keeps the
  video visible with Pause/Stop (click video to toggle pause)

**Typography:** [Sora](https://fonts.google.com/specimen/Sora) (Google Fonts, OFL-1.1) —
pre-baked glyph atlases embedded in the DLL. **Not** the Minecraft default font.

**Look:** Minecraft inventory-style UI — packed-dirt rail, stone panels, raised/inset
bevel buttons, gold accents, 1px text shadow. Square corners (no Discord rounding).

**Display modes:** cursor + wheel hit-testing uses `ClientToScreen` + `GetClientRect` +
viewport scale, so windowed, borderless-windowed, and exclusive fullscreen all work,
including non-1:1 GUI scale / DPI.


### Chat safety & emoji

- **Emoji:** curated Twemoji atlas (CC-BY 4.0), ~280 glyphs + `:shortcodes:` (`:fire:`, `:heart:`, …)
- **Picker:** 😊 button on the chat input opens a scrollable grid
- **Encoding:** UTF-8 end-to-end (`Content-Type: application/json; charset=utf-8`); bodies NFC-normalised on the edge
- **Sanitise (client + proxy + edge):** strip bidi overrides (Trojan Source), C0/C1 controls, tag chars, private-use; keep ZWJ/VS16 for emoji; 500 chars / 2000 bytes cap
- **Rate limit:** 20 messages / 60s per player on `overlay-chat-send`
- **Auth:** friendship required; `toKey`/`playerKey` hex-uuid only

### libmpv video path

1. `vo=libmpv` + OpenGL render context (`mpv_render_context_*`)
2. Each swap-buffers tick renders into an offscreen FBO (640×360 default)
3. UI / PiP blits the colour texture with letterboxing
4. If FBO/extensions are missing → soft fallback to `vo=null` (audio + placeholder)

Place `mpv-2.dll` (from an mpv Windows build) next to the hook DLL or on `PATH`.
yt-dlp/ytdl must be available to libmpv for YouTube resolution.

## Legacy JVM mod

[`bridges/overlay`](../bridges/overlay) Fabric/NeoForge **1.21.1** jar is **not** injected by default.
Set `TUFFBOX_OVERLAY_JVM=1` to also copy the old jar + WATERMeDIA (exact 1.21.1 only).

## Build hook DLL

```powershell
cargo build -p tuffbox-overlay-hook --release
# → target/release/tuffbox_overlay_hook.dll
# Copy next to the desktop binary or set TUFFBOX_OVERLAY_HOOK_DLL
```

Optional: copy `mpv-2.dll` (from an mpv Windows build) beside the hook for playback.

## Settings

Settings → General → **In-game overlay** (`ingameOverlay`) gates session + IPC + inject.

## Deploy (social backend)

```bash
supabase db push   # 016_overlay_social.sql
supabase functions deploy overlay-friends --no-verify-jwt
supabase functions deploy overlay-presence --no-verify-jwt
supabase functions deploy overlay-chat-send --no-verify-jwt
supabase functions deploy overlay-chat-poll --no-verify-jwt
```

## Privacy

Unchanged: presence opt-in, chat requires friendship, edge functions own writes via `writeSecret`.

## Hotkeys

| Key | Action |
|-----|--------|
| F8 | Toggle overlay |
| Esc | Close overlay |
| F9 | Pause / resume media (global) |
| F10 | Stop media (global) |
| ↑ ↓ PgUp PgDn | Scroll focused list |
| Mouse wheel | Scroll under cursor (overlay open) |
