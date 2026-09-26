//! Immediate-mode overlay UI drawn in the SwapBuffers hook.
//! Steam/Discord-style shell: rail + top bar + YouTube / Friends / Chat.

use crate::emoji;
use crate::font;
use crate::gl;
use crate::input;
use crate::ipc;
use crate::lru;
use crate::mpv;
use crate::textutil;
use crate::theme;
use std::collections::HashMap;
use windows::Win32::Foundation::POINT;
use windows::Win32::Graphics::Gdi::HDC;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetAsyncKeyState, VK_BACK, VK_DELETE, VK_DOWN, VK_LBUTTON, VK_NEXT, VK_PRIOR, VK_RETURN, VK_UP,
};
use windows::Win32::UI::WindowsAndMessaging::GetCursorPos;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Tab {
    Youtube,
    Friends,
    Chat,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Focus {
    None,
    FriendAdd,
    ChatInput,
    YtSearch,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum PipCorner {
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
}

/// Flat list row for the friends panel.
enum FriendRow {
    Header(String),
    Friend { friend: ipc::Friend, kind: FriendKind },
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum FriendKind {
    Accepted,
    Incoming,
    Outgoing,
}

pub struct UiState {
    tab: Tab,
    focus: Focus,

    session: ipc::SessionInfo,
    feed: Vec<ipc::FeedItem>,
    friends: ipc::FriendsSnapshot,
    /// All messages we have polled (id-keyed merge).
    messages: Vec<ipc::ChatMessage>,
    chat_cursor: i64,
    /// peer_key → unread count
    unread: HashMap<String, i32>,

    selected_video: Option<String>,
    chat_peer_key: String,
    chat_peer_name: String,

    yt_scroll: f32,
    friends_scroll: f32,
    chat_list_scroll: f32,
    chat_msg_scroll: f32,

    friend_add: String,
    chat_input: String,
    /// Emoji picker popover open (chat tab).
    emoji_picker: bool,
    emoji_picker_scroll: f32,
    yt_query: String,
    status: String,
    hint: String,
    hint_ok: bool,

    /// Picture-in-picture when the full overlay is closed.
    pip_enabled: bool,
    pip_corner: PipCorner,
    pip_scale: u8, // 0=sm 1=md 2=lg

    last_fetch: std::time::Instant,
    last_chat_poll: std::time::Instant,
    char_queue: Vec<char>,
}

impl UiState {
    pub fn new() -> Self {
        Self {
            tab: Tab::Youtube,
            focus: Focus::None,
            session: ipc::SessionInfo::default(),
            feed: Vec::new(),
            friends: ipc::FriendsSnapshot::default(),
            messages: Vec::new(),
            chat_cursor: 0,
            unread: HashMap::new(),
            selected_video: None,
            chat_peer_key: String::new(),
            chat_peer_name: String::new(),
            yt_scroll: 0.0,
            friends_scroll: 0.0,
            chat_list_scroll: 0.0,
            chat_msg_scroll: -1.0, // -1 = pin to bottom
            friend_add: String::new(),
            chat_input: String::new(),
            emoji_picker: false,
            emoji_picker_scroll: 0.0,
            yt_query: String::new(),
            status: "F8 overlay  ·  Esc close  ·  F9 pause  ·  F10 stop  ·  wheel scroll".into(),
            hint: String::new(),
            hint_ok: true,
            pip_enabled: true,
            pip_corner: PipCorner::BottomRight,
            pip_scale: 1,
            last_fetch: std::time::Instant::now() - std::time::Duration::from_secs(60),
            last_chat_poll: std::time::Instant::now() - std::time::Duration::from_secs(60),
            char_queue: Vec::new(),
        }
    }

    /// Called from the low-level keyboard hook for printable input.
    pub fn push_char(&mut self, ch: char) {
        if self.focus == Focus::None {
            return;
        }
        // Cap queue so a stuck hook can't blow memory.
        if self.char_queue.len() < 64 {
            self.char_queue.push(ch);
        }
    }

    pub fn on_open(&mut self) {
        self.refresh_data(true);
        self.poll_chat(true);
    }

    pub fn on_close(&mut self) {
        self.focus = Focus::None;
        self.emoji_picker = false;
    }

    /// Overlay closed: keep media + chat alive, draw PiP if enabled.
    pub fn tick_background(&mut self, hdc: windows::Win32::Graphics::Gdi::HDC) {
        lru::with_textures(|c| c.pump_uploads());
        mpv::tick();
        if self.last_chat_poll.elapsed() > std::time::Duration::from_secs(12) {
            self.poll_chat(false);
        }
        if self.pip_enabled && mpv::is_active() {
            gl::begin_overlay_frame(hdc);
            let (vw, vh) = gl::viewport_size();
            self.draw_pip(vw as f32, vh as f32, cursor(), left_click_edge());
            gl::end_overlay_frame();
        }
    }

    pub fn total_unread(&self) -> i32 {
        self.unread.values().sum()
    }

    pub fn tick_and_draw(&mut self, hdc: HDC) {
        if self.last_fetch.elapsed() > std::time::Duration::from_secs(10) {
            self.refresh_data(false);
        }
        if self.last_chat_poll.elapsed() > std::time::Duration::from_secs(4) {
            self.poll_chat(false);
        }
        lru::with_textures(|c| c.pump_uploads());
        mpv::tick();
        self.drain_text_input();
        self.handle_edit_keys();

        gl::begin_overlay_frame(hdc);
        let (vw, vh) = gl::viewport_size();
        let vw = vw as f32;
        let vh = vh as f32;

        // Dim game behind the shell.
        let bd = theme::BACKDROP;
        gl::fill_rect(0.0, 0.0, vw, vh, bd.0, bd.1, bd.2, bd.3);

        let panel_w = (vw * 0.82).min(1100.0).max(640.0).min(vw - 24.0);
        let panel_h = (vh * 0.84).min(720.0).max(420.0).min(vh - 24.0);
        let px = (vw - panel_w) * 0.5;
        let py = (vh - panel_h) * 0.5;

        // Stone face with dark outer frame (MC container).
        gl::mc_panel(px, py, panel_w, panel_h, theme::CONTENT_BG);

        let mouse = cursor();
        let click = left_click_edge();
        // Wheel notches (positive = up) + keyboard arrows/pgup/pgdn.
        let scroll_delta = input::take_wheel() + scroll_keys();

        self.draw_rail(px, py, panel_h, mouse, click);
        self.draw_topbar(px, py, panel_w);

        let content_x = px + theme::RAIL_W;
        let content_y = py + theme::TOPBAR_H;
        let content_w = panel_w - theme::RAIL_W;
        let content_h = panel_h - theme::TOPBAR_H - theme::STATUS_H;

        match self.tab {
            Tab::Youtube => {
            Tab::Youtube => {
                self.draw_youtube(content_x, content_y, content_w, content_h, mouse, click, scroll_delta)
            }
            Tab::Friends => {
                self.draw_friends(content_x, content_y, content_w, content_h, mouse, click, scroll_delta)
            }
            Tab::Chat => {
                self.draw_chat(content_x, content_y, content_w, content_h, mouse, click, scroll_delta)
            }
        }

        // Status bar (dark dirt strip)
        let st = theme::STATUS_BG;
        gl::dirt_fill(
            px + 2.0,
            py + panel_h - theme::STATUS_H,
            panel_w - 4.0,
            theme::STATUS_H - 2.0,
            st,
        );
        let td = theme::TEXT_DIM;
        font::draw_fit(
            &self.status,
            px + 12.0,
            py + panel_h - theme::STATUS_H + 8.0,
            1.0,
            td.0,
            td.1,
            td.2,
            td.3,
            panel_w - 24.0,
        );

        gl::end_overlay_frame();
    }

    fn draw_rail(&mut self, px: f32, py: f32, panel_h: f32, mouse: (f32, f32), click: bool) {
        let rail_w = theme::RAIL_W;
        // Packed-dirt rail with speckles + raised bevel.
        gl::bevel_rect(px + 2.0, py + 2.0, rail_w - 2.0, panel_h - 4.0, theme::RAIL_BG, true);
        gl::dirt_fill(px + 4.0, py + 4.0, rail_w - 6.0, panel_h - 8.0, theme::RAIL_BG);
        let dv = theme::BORDER_DARK;
        gl::fill_rect(
            px + rail_w - 2.0,
            py + theme::TOPBAR_H,
            2.0,
            panel_h - theme::TOPBAR_H - theme::STATUS_H,
            dv.0,
            dv.1,
            dv.2,
            dv.3,
        );

        let gold = theme::TEXT_GOLD;
        let tm = theme::TEXT_DIM;
        font::draw("TuffBox", px + 14.0, py + 10.0, 1.4, gold.0, gold.1, gold.2, gold.3, 0.0);
        font::draw("OVERLAY", px + 14.0, py + 32.0, 1.0, tm.0, tm.1, tm.2, tm.3, 0.0);

        let tabs = [
            (Tab::Youtube, "YouTube"),
            (Tab::Friends, "Friends"),
            (Tab::Chat, "Chat"),
        ];
        let mut item_y = py + theme::TOPBAR_H + 12.0;
        let item_h = 36.0;
        for (tab, label) in tabs {
            let hot = hit(mouse, px + 8.0, item_y, rail_w - 16.0, item_h);
            let active = self.tab == tab;
            if active {
                gl::bevel_rect(
                    px + 8.0,
                    item_y,
                    rail_w - 16.0,
                    item_h,
                    theme::ACCENT,
                    false, // pressed-in like selected MC button
                );
            } else if hot {
                gl::bevel_rect(
                    px + 8.0,
                    item_y,
                    rail_w - 16.0,
                    item_h,
                    theme::RAIL_HOVER,
                    true,
                );
            } else {
                gl::bevel_rect(
                    px + 8.0,
                    item_y,
                    rail_w - 16.0,
                    item_h,
                    theme::RAIL_ACTIVE,
                    true,
                );
            }
            let color = if active {
                theme::TEXT
            } else {
                theme::TEXT_DIM
            };
            font::draw(
                label,
                px + 20.0,
                item_y + 12.0,
                1.15,
                color.0,
                color.1,
                color.2,
                color.3,
                rail_w - 48.0,
            );

            if tab == Tab::Chat {
                let unread = self.total_unread();
                if unread > 0 {
                    let badge = if unread > 99 {
                        "99+".to_string()
                    } else {
                        unread.to_string()
                    };
                    let bw = font::measure(&badge, 1.0) + 10.0;
                    let bx = px + rail_w - 16.0 - bw;
                    let by = item_y + 10.0;
                    let dg = theme::DANGER;
                    gl::fill_rect(bx, by, bw, 16.0, dg.0, dg.1, dg.2, dg.3);
                    font::draw(
                        &badge,
                        bx + 5.0,
                        by + 4.0,
                        1.0,
                        1.0,
                        1.0,
                        1.0,
                        1.0,
                        0.0,
                    );
                }
            }

            if hot && click {
                self.tab = tab;
                self.focus = Focus::None;
                if tab == Tab::Chat && !self.chat_peer_key.is_empty() {
                    self.unread.remove(&self.chat_peer_key);
                }
            }
            item_y += item_h + 4.0;
        }

        // Footer hints
        let foot_y = py + panel_h - theme::STATUS_H - 40.0;
        font::draw("F8 close", px + 14.0, foot_y + 16.0, 1.0, tm.0, tm.1, tm.2, tm.3, 0.0);
        if mpv::is_active() {
            let sc = theme::SUCCESS;
            font::draw(
                "playing",
                px + 14.0,
                foot_y,
                1.0,
                sc.0,
                sc.1,
                sc.2,
                sc.3,
                0.0,
            );
        }
    }

    fn draw_topbar(&self, px: f32, py: f32, panel_w: f32) {
        let rail_w = theme::RAIL_W;
        gl::dirt_fill(
            px + rail_w,
            py + 2.0,
            panel_w - rail_w - 2.0,
            theme::TOPBAR_H - 2.0,
            theme::TOPBAR_BG,
        );
        // gold underline
        let g = theme::GOLD;
        gl::fill_rect(
            px + rail_w,
            py + theme::TOPBAR_H - 2.0,
            panel_w - rail_w - 2.0,
            2.0,
            g.0,
            g.1,
            g.2,
            0.85,
        );

        let title = match self.tab {
            Tab::Youtube => "YouTube",
            Tab::Friends => "Friends",
            Tab::Chat => "Chat",
        };
        let t = theme::TEXT;
        font::draw(
            title,
            px + rail_w + 16.0,
            py + 14.0,
            1.25,
            t.0,
            t.1,
            t.2,
            t.3,
            0.0,
        );

        let td = theme::TEXT_DIM;
        let mut right = px + panel_w - 16.0;
        if !self.session.username.is_empty() {
            let label = format!("{}  ·", self.session.username);
            let w = font::measure(&label, 1.0);
            right -= w;
            font::draw(&label, right, py + 14.0, 1.0, td.0, td.1, td.2, td.3, 0.0);
        }
        if !self.session.pack_name.is_empty() {
            let pack = &self.session.pack_name;
            let w = font::measure(pack, 1.0) + 16.0;
            right -= w;
            font::draw_fit(pack, right, py + 14.0, 1.0, td.0, td.1, td.2, td.3, w - 8.0);
        }
    }

    // ── YouTube ───────────────────────────────────────────────────────

    fn filtered_feed(&self) -> Vec<&ipc::FeedItem> {
        let q = self.yt_query.trim().to_lowercase();
        if q.is_empty() {
            return self.feed.iter().collect();
        }
        self.feed
            .iter()
            .filter(|it| {
                it.title.to_lowercase().contains(&q)
                    || it.channel.to_lowercase().contains(&q)
                    || it.id.to_lowercase().contains(&q)
            })
            .collect()
    }

    fn draw_youtube(
        &mut self,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        mouse: (f32, f32),
        click: bool,
        scroll_delta: f32,
    ) {
        let pad = 14.0;

        // Search bar
        let search_h = 28.0;
        let search_x = x + pad;
        let search_y = y + pad;
        let search_w = (w * 0.55).min(420.0).max(180.0);
        let focused = self.focus == Focus::YtSearch;
        gl::bevel_rect(search_x, search_y, search_w, search_h, theme::INPUT_BG, false);
        if focused {
            let g = theme::GOLD;
            gl::fill_rect(search_x, search_y + search_h - 1.0, search_w, 1.0, g.0, g.1, g.2, 0.9);
        }
        let placeholder = self.yt_query.is_empty() && !focused;
        let shown = if placeholder {
            "Filter feed or paste youtube.com/watch?v=..."
        } else {
            &self.yt_query
        };
        let color = if placeholder {
            theme::TEXT_MUTED
        } else {
            theme::TEXT
        };
        font::draw_fit(
            shown,
            search_x + 10.0,
            search_y + 9.0,
            1.05,
            color.0,
            color.1,
            color.2,
            color.3,
            search_w - 20.0,
        );
        if focused
            && (std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_millis() / 500 % 2 == 0)
                .unwrap_or(false))
        {
            let caret_x =
                search_x + 10.0 + font::measure(&self.yt_query, 1.05).min(search_w - 24.0);
            let t = theme::TEXT;
            gl::fill_rect(caret_x, search_y + 6.0, 2.0, 16.0, t.0, t.1, t.2, t.3);
        }
        if click {
            if hit(mouse, search_x, search_y, search_w, search_h) {
                self.focus = Focus::YtSearch;
            } else if self.focus == Focus::YtSearch
                && !hit(
                    mouse,
                    search_x + search_w + 8.0,
                    search_y,
                    200.0,
                    search_h,
                )
            {
                // keep focus only if clicking play/clear nearby
            }
        }

        let mut bx = search_x + search_w + 8.0;
        if self.draw_btn("Play URL", bx, search_y, search_h, true, mouse, click) {
            self.play_from_query();
        }
        bx += font::measure("Play URL", 1.05) + 28.0;
        if self.draw_btn("Clear", bx, search_y, search_h, false, mouse, click) {
            self.yt_query.clear();
            self.yt_scroll = 0.0;
        }
        bx += font::measure("Clear", 1.05) + 28.0;
        // PiP toggles live on the YouTube page too
        let pip_label = if self.pip_enabled { "PiP: ON" } else { "PiP: off" };
        if self.draw_btn(pip_label, bx, search_y, search_h, false, mouse, click) {
            self.pip_enabled = !self.pip_enabled;
        }
        bx += font::measure(pip_label, 1.05) + 28.0;
        if self.draw_btn("Corner", bx, search_y, search_h, false, mouse, click) {
            self.pip_corner = match self.pip_corner {
                PipCorner::TopLeft => PipCorner::TopRight,
                PipCorner::TopRight => PipCorner::BottomRight,
                PipCorner::BottomRight => PipCorner::BottomLeft,
                PipCorner::BottomLeft => PipCorner::TopLeft,
            };
        }
        bx += font::measure("Corner", 1.05) + 28.0;
        if self.draw_btn("Size", bx, search_y, search_h, false, mouse, click) {
            self.pip_scale = (self.pip_scale + 1) % 3;
        }

        // Layout: video surface left, feed list right
        let mid_y = search_y + search_h + 12.0;
        let right_w = (w * 0.38).max(240.0).min(360.0);
        let list_x = x + w - pad - right_w;
        let list_y = mid_y;
        let list_w = right_w;
        let list_h = h - (list_y - y) - pad;

        let video_x = x + pad;
        let video_y = mid_y;
        let video_w = (list_x - pad - video_x).max(280.0);
        let mut video_h = (video_w * 9.0 / 16.0).max(140.0);
        let max_vh = h - (mid_y - y) - 80.0;
        if video_h > max_vh {
            video_h = max_vh.max(120.0);
        }

        // Video surface (dark inset frame)
        gl::bevel_rect(video_x - 2.0, video_y - 2.0, video_w + 4.0, video_h + 4.0, (0.08, 0.08, 0.08, 1.0), false);
        self.draw_video_surface(video_x, video_y, video_w, video_h, mouse, click);

        // Transport under video
        let ty = video_y + video_h + 8.0;
        let (time, dur, vol) = mpv::time_info();
        // Seek bar
        let seek_h = 10.0;
        let pbg = theme::PANEL_BG;
        gl::fill_rect(video_x, ty + 4.0, video_w, seek_h, pbg.0, pbg.1, pbg.2, pbg.3);
        if dur > 0.0 {
            let frac = (time / dur).clamp(0.0, 1.0) as f32;
            let ac = theme::ACCENT;
            gl::fill_rect(video_x, ty + 4.0, video_w * frac, seek_h, ac.0, ac.1, ac.2, ac.3);
            if click && hit(mouse, video_x, ty, video_w, seek_h + 8.0) {
                let f = ((mouse.0 - video_x) / video_w).clamp(0.0, 1.0) as f64;
                let target = f * dur;
                let rel = target - time;
                mpv::seek_relative(rel);
            }
        }
        let td = theme::TEXT_DIM;
        let clock = format!(
            "{} / {}   vol {}",
            fmt_clock(time),
            if dur > 0.0 { fmt_clock(dur) } else { "--:--".into() },
            vol
        );
        font::draw(
            &clock,
            video_x,
            ty + seek_h + 10.0,
            1.0,
            td.0,
            td.1,
            td.2,
            td.3,
            0.0,
        );

        let mut tbtn_x = video_x + font::measure(&clock, 1.0) + 16.0;
        let tbtn_y = ty + seek_h + 4.0;
        let pause_label = if mpv::is_paused() { "Play" } else { "Pause" };
        if mpv::is_active() {
            if self.draw_btn(pause_label, tbtn_x, tbtn_y, 22.0, true, mouse, click) {
                mpv::toggle_pause();
            }
            tbtn_x += font::measure(pause_label, 1.05) + 28.0;
            if self.draw_btn("Stop", tbtn_x, tbtn_y, 22.0, false, mouse, click) {
                mpv::stop();
                self.selected_video = None;
                self.status = "Playback stopped".into();
            }
            tbtn_x += font::measure("Stop", 1.05) + 28.0;
            if self.draw_btn("Vol-", tbtn_x, tbtn_y, 22.0, false, mouse, click) {
                mpv::volume_delta(-10);
            }
            tbtn_x += font::measure("Vol-", 1.05) + 28.0;
            if self.draw_btn("Vol+", tbtn_x, tbtn_y, 22.0, false, mouse, click) {
                mpv::volume_delta(10);
            }
        }

        // Feed list
        let rows = self.filtered_feed();
        gl::bevel_rect(list_x, list_y, list_w, list_h, theme::PANEL_BG, true);
        let t = theme::TEXT;
        let header = if self.yt_query.trim().is_empty() {
            format!("Minecraft feed ({})", self.feed.len())
        } else {
            format!("Matches {} / {}", rows.len(), self.feed.len())
        };
        font::draw(
            &header,
            list_x + 8.0,
            list_y + 8.0,
            1.05,
            t.0,
            t.1,
            t.2,
            t.3,
            list_w - 16.0,
        );

        let row_h = 64.0;
        let gap = 6.0;
        let list_top = list_y + 28.0;
        let list_inner_h = list_h - 36.0;
        let total_h = rows.len() as f32 * (row_h + gap);
        let max_scroll = (total_h - list_inner_h).max(0.0);
        if hit(mouse, list_x, list_top, list_w, list_inner_h) {
            self.yt_scroll = (self.yt_scroll - scroll_delta * 40.0).clamp(0.0, max_scroll);
        }

        let mut cy = list_top - self.yt_scroll;
        let mut play_id: Option<(String, String)> = None;
        for item in &rows {
            if cy + row_h < list_top || cy > list_top + list_inner_h {
                cy += row_h + gap;
                continue;
            }
            let hot = hit(mouse, list_x + 4.0, cy, list_w - 8.0, row_h);
            if hot {
                let rh = theme::RAIL_HOVER;
                gl::fill_rect(list_x + 4.0, cy, list_w - 8.0, row_h, rh.0, rh.1, rh.2, rh.3);
            }
            let thumb = lru::with_textures(|c| c.get_or_request(&item.id, &item.thumbnail_url))
                .flatten()
                .unwrap_or(0);
            gl::textured_rect(thumb, list_x + 10.0, cy + 6.0, 80.0, 48.0);
            let t = theme::TEXT;
            let td = theme::TEXT_DIM;
            font::draw_fit(
                &item.title,
                list_x + 100.0,
                cy + 12.0,
                1.05,
                t.0,
                t.1,
                t.2,
                t.3,
                list_w - 120.0,
            );
            font::draw_fit(
                &item.channel,
                list_x + 100.0,
                cy + 32.0,
                1.0,
                td.0,
                td.1,
                td.2,
                td.3,
                list_w - 120.0,
            );
            if hot && click {
                play_id = Some((item.id.clone(), item.title.clone()));
            }
            cy += row_h + gap;
        }
        if rows.is_empty() {
            let tm = theme::TEXT_MUTED;
            let msg = if self.feed.is_empty() {
                "No feed items"
            } else {
                "No matches"
            };
            font::draw(msg, list_x + 12.0, list_top + 12.0, 1.1, tm.0, tm.1, tm.2, tm.3, 0.0);
        }
        if let Some((id, title)) = play_id {
            self.play_video(&id, &title);
        }
    }

    fn draw_video_surface(
        &self,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        _mouse: (f32, f32),
        _click: bool,
    ) {
        if let Some((tex, pw, ph)) = mpv::texture() {
            gl::textured_rect_fit(tex, x, y, w, h, pw as f32, ph as f32);
            let title = mpv::title();
            if !title.is_empty() {
                gl::fill_rect(x, y, w, 18.0, 0.0, 0.0, 0.0, 0.55);
                let t = theme::TEXT;
                font::draw_fit(&title, x + 8.0, y + 5.0, 1.05, t.0, t.1, t.2, t.3, w - 16.0);
            }
            if mpv::is_paused() {
                let tm = theme::TEXT;
                let label = "PAUSED";
                let lw = font::measure(label, 1.4);
                gl::fill_rect(
                    x + (w - lw) * 0.5 - 12.0,
                    y + h * 0.5 - 14.0,
                    lw + 24.0,
                    28.0,
                    0.0,
                    0.0,
                    0.0,
                    0.55,
                );
                font::draw(
                    label,
                    x + (w - lw) * 0.5,
                    y + h * 0.5 - 6.0,
                    1.4,
                    tm.0,
                    tm.1,
                    tm.2,
                    tm.3,
                    0.0,
                );
            }
        } else {
            gl::fill_rect(x, y, w, h, 0.0, 0.0, 0.0, 1.0);
            let tm = theme::TEXT_MUTED;
            let line1 = "Nothing playing";
            let line2 = "Pick a video or paste a YouTube URL";
            let w1 = font::measure(line1, 1.2);
            let w2 = font::measure(line2, 1.0);
            font::draw(
                line1,
                x + (w - w1) * 0.5,
                y + h * 0.5 - 14.0,
                1.2,
                tm.0,
                tm.1,
                tm.2,
                tm.3,
                0.0,
            );
            font::draw(
                line2,
                x + (w - w2) * 0.5,
                y + h * 0.5 + 6.0,
                1.0,
                tm.0,
                tm.1,
                tm.2,
                tm.3,
                0.0,
            );
        }
    }

    fn play_video(&mut self, id: &str, title: &str) {
        self.selected_video = Some(id.to_string());
        if let Some(url) = ipc::resolve_youtube(id) {
            match mpv::play_url_titled(&url, title) {
                Ok(()) => {
                    mpv::set_title(title);
                    self.status = format!("Playing {title}");
                }
                Err(e) => self.status = e,
            }
        } else {
            self.status = "Could not resolve YouTube URL".into();
        }
    }

    fn play_from_query(&mut self) {
        let raw = self.yt_query.trim().to_string();
        if raw.is_empty() {
            return;
        }
        // Extract video id from common URL shapes, else treat as filter-only.
        if let Some(id) = extract_youtube_id(&raw) {
            self.play_video(&id, &raw);
            return;
        }
        if raw.starts_with("http://") || raw.starts_with("https://") {
            match mpv::play_url_titled(&raw, &raw) {
                Ok(()) => self.status = format!("Playing {raw}"),
                Err(e) => self.status = e,
            }
            return;
        }
        // Otherwise just apply filter (already live via yt_query).
        self.yt_scroll = 0.0;
        self.status = format!("Filter: {raw}");
    }

    /// Picture-in-picture widget drawn while the full overlay is closed.
    fn draw_pip(&mut self, vw: f32, vh: f32, mouse: (f32, f32), click: bool) {
        let Some((tex, pw, ph)) = mpv::texture() else {
            return;
        };
        let widths = [192.0f32, 288.0, 384.0];
        let pip_w = widths[self.pip_scale as usize % 3];
        let pip_h = pip_w * 9.0 / 16.0;
        let margin = 14.0;
        let (px, py) = match self.pip_corner {
            PipCorner::TopLeft => (margin, margin),
            PipCorner::TopRight => (vw - pip_w - margin, margin),
            PipCorner::BottomLeft => (margin, vh - pip_h - margin - 28.0),
            PipCorner::BottomRight => (vw - pip_w - margin, vh - pip_h - margin - 28.0),
        };
        let pb = theme::TOPBAR_BG;
        gl::fill_rect(px - 4.0, py - 4.0, pip_w + 8.0, pip_h + 32.0, pb.0, pb.1, pb.2, 0.92);
        gl::textured_rect_fit(tex, px, py, pip_w, pip_h, pw as f32, ph as f32);

        let title = mpv::title();
        if !title.is_empty() {
            gl::fill_rect(px, py, pip_w, 16.0, 0.0, 0.0, 0.0, 0.55);
            let t = theme::TEXT;
            font::draw_fit(&title, px + 6.0, py + 4.0, 1.0, t.0, t.1, t.2, t.3, pip_w - 12.0);
        }

        let by = py + pip_h + 4.0;
        let pause_l = if mpv::is_paused() { "Play" } else { "Pause" };
        if self.draw_btn(pause_l, px, by, 20.0, true, mouse, click) {
            mpv::toggle_pause();
        }
        let stop_x = px + font::measure(pause_l, 1.05) + 28.0;
        if self.draw_btn("Stop", stop_x, by, 20.0, false, mouse, click) {
            mpv::stop();
        }
        // Click on video toggles pause too.
        if click && hit(mouse, px, py, pip_w, pip_h) {
            mpv::toggle_pause();
        }
    }

    // ── Friends ───────────────────────────────────────────────────────

    fn friend_rows(&self) -> Vec<FriendRow> {
        let mut rows = Vec::new();
        if !self.friends.incoming.is_empty() {
            rows.push(FriendRow::Header(format!(
                "Requests ({})",
                self.friends.incoming.len()
            )));
            for f in &self.friends.incoming {
                rows.push(FriendRow::Friend {
                    friend: f.clone(),
                    kind: FriendKind::Incoming,
                });
            }
        }
        rows.push(FriendRow::Header(format!(
            "Friends ({})",
            self.friends.friends.len()
        )));
        for f in &self.friends.friends {
            rows.push(FriendRow::Friend {
                friend: f.clone(),
                kind: FriendKind::Accepted,
            });
        }
        if !self.friends.outgoing.is_empty() {
            rows.push(FriendRow::Header("Outgoing".into()));
            for f in &self.friends.outgoing {
                rows.push(FriendRow::Friend {
                    friend: f.clone(),
                    kind: FriendKind::Outgoing,
                });
            }
        }
        rows
    }

    fn draw_friends(
        &mut self,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        mouse: (f32, f32),
        click: bool,
        scroll_delta: f32,
    ) {
        let pad = 16.0;
        // Content sits on the shell stone face — no extra full fill.

        if !self.friends.error.is_empty() && !self.friends.ok {
            let warn = theme::WARNING;
            font::draw_fit(
                &format!("Friends unavailable: {}", self.friends.error),
                x + pad,
                y + h * 0.45,
                1.1,
                warn.0,
                warn.1,
                warn.2,
                warn.3,
                w - pad * 2.0,
            );
            let tm = theme::TEXT_MUTED;
            font::draw(
                "Launch via TuffBox with a signed-in account.",
                x + pad,
                y + h * 0.45 + 20.0,
                1.0,
                tm.0,
                tm.1,
                tm.2,
                tm.3,
                0.0,
            );
            return;
        }

        // Add-friend input
        let input_x = x + pad;
        let input_y = y + pad;
        let input_h = 28.0;
        let input_w = (w * 0.42).min(280.0).max(160.0);
        let focused = self.focus == Focus::FriendAdd;
        gl::bevel_rect(input_x, input_y, input_w, input_h, theme::INPUT_BG, false);
        if focused {
            let g = theme::GOLD;
            gl::fill_rect(input_x, input_y, input_w, 1.0, g.0, g.1, g.2, 0.9);
            gl::fill_rect(
                input_x,
                input_y + input_h - 1.0,
                input_w,
                1.0,
                g.0,
                g.1,
                g.2,
                0.9,
            );
        }
        let placeholder = self.friend_add.is_empty() && !focused;
        let shown = if placeholder {
            "Add friend by username..."
        } else {
            &self.friend_add
        };
        let color = if placeholder {
            theme::TEXT_MUTED
        } else {
            theme::TEXT
        };
        font::draw_fit(
            shown,
            input_x + 10.0,
            input_y + 9.0,
            1.1,
            color.0,
            color.1,
            color.2,
            color.3,
            input_w - 20.0,
        );
        if focused && (std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() / 500 % 2 == 0)
            .unwrap_or(false))
        {
            let caret_x = input_x + 10.0 + font::measure(&self.friend_add, 1.1).min(input_w - 24.0);
            let t = theme::TEXT;
            gl::fill_rect(caret_x, input_y + 6.0, 2.0, 16.0, t.0, t.1, t.2, t.3);
        }

        // Buttons
        let mut bx = input_x + input_w + 10.0;
        let send_w = font::measure("Send request", 1.05) + 20.0;
        let refresh_w = font::measure("Refresh", 1.05) + 20.0;
        if click {
            if hit(mouse, input_x, input_y, input_w, input_h) {
                self.focus = Focus::FriendAdd;
            } else if !hit(mouse, bx, input_y, send_w + 8.0 + refresh_w, 28.0) {
                // Click on chrome (not input, not buttons) clears focus.
                if self.focus == Focus::FriendAdd {
                    self.focus = Focus::None;
                }
            }
        }
        if self.draw_btn("Send request", bx, input_y, 28.0, true, mouse, click) {
            self.submit_add_friend();
        }
        bx += send_w + 8.0;
        if self.draw_btn("Refresh", bx, input_y, 28.0, false, mouse, click) {
            self.refresh_data(true);
            self.hint = "Refreshed.".into();
            self.hint_ok = true;
        }

        if !self.hint.is_empty() {
            let hc = if self.hint_ok {
                theme::SUCCESS
            } else {
                theme::WARNING
            };
            font::draw_fit(
                &self.hint,
                x + pad,
                input_y + 34.0,
                1.0,
                hc.0,
                hc.1,
                hc.2,
                hc.3,
                w - pad * 2.0,
            );
        }

        // List
        let list_x = x + pad;
        let list_y = input_y + 52.0;
        let list_w = w - pad * 2.0;
        let list_h = h - (list_y - y) - pad;
        let row_h = 40.0;

        let rows = self.friend_rows();
        let total_h = rows.len() as f32 * row_h;
        let max_scroll = (total_h - list_h).max(0.0);
        if hit(mouse, list_x, list_y, list_w, list_h) {
            self.friends_scroll = (self.friends_scroll - scroll_delta * 36.0).clamp(0.0, max_scroll);
        }

        let mut cy = list_y - self.friends_scroll;
        // Collect click actions to apply after borrow ends.
        let mut action: Option<FriendAction> = None;

        for row in &rows {
            if cy + row_h < list_y || cy > list_y + list_h {
                cy += row_h;
                continue;
            }
            match row {
                FriendRow::Header(label) => {
                    let tm = theme::TEXT_MUTED;
                    font::draw(
                        label,
                        list_x + 4.0,
                        cy + 14.0,
                        1.05,
                        tm.0,
                        tm.1,
                        tm.2,
                        tm.3,
                        list_w - 8.0,
                    );
                }
                FriendRow::Friend { friend, kind } => {
                    let hot = hit(mouse, list_x, cy, list_w, row_h);
                    if hot {
                        let rh = theme::ROW_HOVER;
                        gl::fill_rect(list_x, cy, list_w, row_h, rh.0, rh.1, rh.2, rh.3);
                    }
                    // Presence dot
                    let dot = if friend.online {
                        theme::SUCCESS
                    } else {
                        theme::TEXT_MUTED
                    };
                    gl::fill_rect(list_x + 8.0, cy + 14.0, 10.0, 10.0, dot.0, dot.1, dot.2, dot.3);

                    let t = theme::TEXT;
                    let name = if friend.name.is_empty() {
                        &friend.key
                    } else {
                        &friend.name
                    };
                    font::draw_fit(
                        name,
                        list_x + 28.0,
                        cy + 8.0,
                        1.15,
                        t.0,
                        t.1,
                        t.2,
                        t.3,
                        list_w - 220.0,
                    );

                    let sub = if friend.online {
                        if !friend.pack.is_empty() {
                            let mut s = format!("Playing {}", friend.pack);
                            if !friend.server.is_empty() {
                                s.push_str(" · ");
                                s.push_str(&friend.server);
                            }
                            s
                        } else {
                            "Online".into()
                        }
                    } else {
                        "Offline".into()
                    };
                    let td = if friend.online {
                        theme::TEXT_DIM
                    } else {
                        theme::TEXT_MUTED
                    };
                    font::draw_fit(
                        &sub,
                        list_x + 28.0,
                        cy + 24.0,
                        1.0,
                        td.0,
                        td.1,
                        td.2,
                        td.3,
                        list_w - 220.0,
                    );

                    // Action buttons right-to-left
                    match kind {
                        FriendKind::Incoming => {
                            let (decline_clicked, e1) =
                                self.row_btn_hit("X", list_x + list_w - 8.0, cy + 10.0, mouse, click);
                            let (accept_clicked, _) =
                                self.row_btn_hit("OK", e1, cy + 10.0, mouse, click);
                            if decline_clicked {
                                action = Some(FriendAction::Remove(friend.id));
                            } else if accept_clicked {
                                action = Some(FriendAction::Accept(friend.id));
                            }
                        }
                        FriendKind::Accepted => {
                            let (rm_clicked, e1) =
                                self.row_btn_hit("X", list_x + list_w - 8.0, cy + 10.0, mouse, click);
                            let (chat_clicked, _) =
                                self.row_btn_hit("Chat", e1, cy + 10.0, mouse, click);
                            if chat_clicked {
                                action = Some(FriendAction::OpenChat {
                                    key: friend.key.clone(),
                                    name: name.to_string(),
                                });
                            } else if rm_clicked {
                                action = Some(FriendAction::Remove(friend.id));
                            }
                        }
                        FriendKind::Outgoing => {
                            let (rm_clicked, _) =
                                self.row_btn_hit("X", list_x + list_w - 8.0, cy + 10.0, mouse, click);
                            if rm_clicked {
                                action = Some(FriendAction::Remove(friend.id));
                            }
                        }
                    }
                }
            }
            cy += row_h;
        }

        // Mini-player
        if let Some((tex, pw, ph)) = mpv::texture() {
            let pw = pw as f32;
            let ph = ph as f32;
            let mx = x + w - pw - 12.0;
            let my = y + h - ph - 48.0;
            gl::fill_rect(
                mx - 8.0,
                my - 8.0,
                pw + 16.0,
                ph + 40.0,
                0.05,
                0.05,
                0.08,
                0.95,
            );
            gl::textured_rect(tex, mx, my, pw, ph);
            // Transport hit targets
            let pause = hit(mouse, mx, my + ph + 4.0, 60.0, 22.0);
            let stop = hit(mouse, mx + 70.0, my + ph + 4.0, 60.0, 22.0);
            gl::fill_rect(mx, my + ph + 4.0, 60.0, 22.0, 0.2, 0.5, 0.3, 1.0);
            gl::fill_rect(mx + 70.0, my + ph + 4.0, 60.0, 22.0, 0.5, 0.2, 0.2, 1.0);
            if click && pause {
                mpv::toggle_pause();
            }
            if click && stop {
                mpv::stop();
                self.selected = None;
            }
        }
        if let Some(act) = action {
            self.apply_friend_action(act);
        }
        }
    }

    fn apply_friend_action(&mut self, act: FriendAction) {
        match act {
            FriendAction::Accept(id) => match ipc::friends_accept(id) {
                Ok(_) => {
                    self.hint = "Friend accepted.".into();
                    self.hint_ok = true;
                    self.refresh_data(true);
                }
                Err(e) => {
                    self.hint = e;
                    self.hint_ok = false;
                }
            },
            FriendAction::Remove(id) => match ipc::friends_remove(id) {
                Ok(_) => {
                    self.hint = "Removed.".into();
                    self.hint_ok = true;
                    self.refresh_data(true);
                }
                Err(e) => {
                    self.hint = e;
                    self.hint_ok = false;
                }
            },
            FriendAction::OpenChat { key, name } => {
                self.tab = Tab::Chat;
                self.chat_peer_key = key.clone();
                self.chat_peer_name = name;
                self.chat_msg_scroll = -1.0; // pin to bottom
                self.unread.remove(&key);
                self.focus = Focus::ChatInput;
                self.status = format!("Chat with {}", self.chat_peer_name);
            }
        }
    }

    fn submit_add_friend(&mut self) {
        let name = self.friend_add.trim().to_string();
        if name.is_empty() {
            return;
        }
        if !textutil::is_safe_username(&name) {
            self.hint = "Username: 1-32 letters, digits, _ - .".into();
            self.hint_ok = false;
            return;
        }
        self.friend_add.clear();
        match ipc::friends_add(&name) {
            Ok(v) => {
                if v.get("accepted").and_then(|a| a.as_bool()).unwrap_or(false) {
                    self.hint = format!("{name} accepted — you are friends now.");
                    self.hint_ok = true;
                } else if v.get("already").and_then(|a| a.as_bool()).unwrap_or(false) {
                    let st = v
                        .get("status")
                        .and_then(|s| s.as_str())
                        .unwrap_or("pending");
                    self.hint = format!("Already {st}.");
                    self.hint_ok = true;
                } else if let Some(err) = v.get("error").and_then(|e| e.as_str()) {
                    self.hint = err.to_string();
                    self.hint_ok = false;
                } else {
                    self.hint = format!("Request sent to {name}.");
                    self.hint_ok = true;
                }
                self.refresh_data(true);
            }
            Err(e) => {
                self.hint = e;
                self.hint_ok = false;
            }
        }
    }

    // ── Chat ──────────────────────────────────────────────────────────

    fn draw_chat(
        &mut self,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        mouse: (f32, f32),
        click: bool,
        scroll_delta: f32,
    ) {
        let list_w = 200.0f32.min(w * 0.32).max(150.0);
        gl::bevel_rect(x + 2.0, y + 2.0, list_w - 4.0, h - 4.0, theme::PANEL_BG, true);
        let dv = theme::BORDER_DARK;
        gl::fill_rect(x + list_w - 2.0, y + 4.0, 2.0, h - 8.0, dv.0, dv.1, dv.2, 0.6);

        let tm = theme::TEXT_MUTED;
        font::draw(
            "Direct messages",
            x + 12.0,
            y + 12.0,
            1.05,
            tm.0,
            tm.1,
            tm.2,
            tm.3,
            list_w - 24.0,
        );

        // Conversation list
        let friends = self.friends.friends.clone();
        let row_h = 34.0;
        let top = y + 34.0;
        let list_h = h - 40.0;
        let total_h = friends.len() as f32 * row_h;
        let max_scroll = (total_h - list_h).max(0.0);
        if hit(mouse, x, top, list_w, list_h) {
            self.chat_list_scroll =
                (self.chat_list_scroll - scroll_delta * 32.0).clamp(0.0, max_scroll);
        }

        let mut open_peer: Option<(String, String)> = None;
        let mut cy = top - self.chat_list_scroll;
        for f in &friends {
            if cy + row_h < top || cy > top + list_h {
                cy += row_h;
                continue;
            }
            let selected = f.key == self.chat_peer_key;
            let hot = hit(mouse, x + 4.0, cy, list_w - 8.0, row_h);
            if selected {
                let ra = theme::ROW_SELECTED;
                gl::fill_rect(x + 4.0, cy, list_w - 9.0, row_h, ra.0, ra.1, ra.2, ra.3);
            } else if hot {
                let rh = theme::RAIL_HOVER;
                gl::fill_rect(x + 4.0, cy, list_w - 9.0, row_h, rh.0, rh.1, rh.2, rh.3);
            }
            let dot = if f.online {
                theme::SUCCESS
            } else {
                theme::TEXT_MUTED
            };
            gl::fill_rect(x + 14.0, cy + 12.0, 8.0, 8.0, dot.0, dot.1, dot.2, dot.3);

            let name = if f.name.is_empty() { &f.key } else { &f.name };
            let color = if selected {
                theme::TEXT
            } else {
                theme::TEXT_DIM
            };
            font::draw_fit(
                name,
                x + 28.0,
                cy + 12.0,
                1.1,
                color.0,
                color.1,
                color.2,
                color.3,
                list_w - 70.0,
            );

            let unread = *self.unread.get(&f.key).unwrap_or(&0);
            if unread > 0 && !selected {
                let badge = if unread > 99 {
                    "99+".to_string()
                } else {
                    unread.to_string()
                };
                let bw = font::measure(&badge, 1.0) + 10.0;
                let bx = x + list_w - 14.0 - bw;
                let dg = theme::DANGER;
                gl::fill_rect(bx, cy + 9.0, bw, 16.0, dg.0, dg.1, dg.2, dg.3);
                font::draw(&badge, bx + 5.0, cy + 13.0, 1.0, 1.0, 1.0, 1.0, 1.0, 0.0);
            }

            if hot && click {
                open_peer = Some((f.key.clone(), name.to_string()));
            }
            cy += row_h;
        }

        if friends.is_empty() {
            font::draw(
                "No friends yet.",
                x + 12.0,
                top + 8.0,
                1.0,
                tm.0,
                tm.1,
                tm.2,
                tm.3,
                0.0,
            );
            font::draw(
                "Add some on Friends.",
                x + 12.0,
                top + 24.0,
                1.0,
                tm.0,
                tm.1,
                tm.2,
                tm.3,
                0.0,
            );
        }

        if let Some((key, name)) = open_peer {
            self.chat_peer_key = key.clone();
            self.chat_peer_name = name;
            self.chat_msg_scroll = -1.0;
            self.unread.remove(&key);
            self.focus = Focus::ChatInput;
        }

        // Messages pane
        let msg_x = x + list_w + 12.0;
        let msg_w = w - list_w - 24.0;
        let input_h = 28.0;
        let input_y = y + h - 12.0 - input_h;
        let msg_y = y + 28.0;
        let msg_h = input_y - msg_y - 10.0;

        if self.chat_peer_key.is_empty() {
            let tm = theme::TEXT_MUTED;
            let label = "Select a friend to start chatting";
            let lw = font::measure(label, 1.15);
            font::draw(
                label,
                msg_x + (msg_w - lw) * 0.5,
                msg_y + msg_h * 0.45,
                1.15,
                tm.0,
                tm.1,
                tm.2,
                tm.3,
                0.0,
            );
        } else {
            let t = theme::TEXT;
            font::draw(
                &format!("@ {}", self.chat_peer_name),
                msg_x,
                y + 10.0,
                1.15,
                t.0,
                t.1,
                t.2,
                t.3,
                msg_w,
            );
            gl::fill_rect(msg_x, msg_y - 4.0, msg_w, 1.0, dv.0, dv.1, dv.2, dv.3);

            let my_key = self.session.uuid.clone();
            let msgs: Vec<_> = self
                .messages
                .iter()
                .filter(|m| {
                    (m.from_key == self.chat_peer_key && m.to_key == my_key)
                        || (m.to_key == self.chat_peer_key && m.from_key == my_key)
                        || conversation_matches(&m.conversation, &my_key, &self.chat_peer_key)
                })
                .cloned()
                .collect();

            // Flatten to lines
            let line_h = 14.0;
            let mut lines: Vec<(String, (f32, f32, f32, f32))> = Vec::new();
            for m in &msgs {
                let mine = m.from_key == my_key;
                let header_c = if mine {
                    theme::ACCENT
                } else {
                    theme::SUCCESS
                };
                let who = if m.from_name.is_empty() {
                    if mine {
                        "You".into()
                    } else {
                        self.chat_peer_name.clone()
                    }
                } else {
                    m.from_name.clone()
                };
                lines.push((who, header_c));
                for body_line in font::wrap(&m.body, 1.05, msg_w - 24.0) {
                    lines.push((body_line, theme::TEXT));
                }
                lines.push((String::new(), theme::TEXT)); // spacer
            }

            let total_h = lines.len() as f32 * line_h;
            let max_scroll = (total_h - msg_h).max(0.0);
            // Default pin to bottom: scroll = max when user hasn't scrolled up.
            if self.chat_msg_scroll < 0.0 {
                self.chat_msg_scroll = max_scroll;
            }
            if hit(mouse, msg_x, msg_y, msg_w, msg_h) {
                self.chat_msg_scroll =
                    (self.chat_msg_scroll - scroll_delta * 28.0).clamp(0.0, max_scroll);
            } else {
                self.chat_msg_scroll = self.chat_msg_scroll.clamp(0.0, max_scroll);
            }

            let first = (self.chat_msg_scroll / line_h).floor() as usize;
            let visible = ((msg_h / line_h).ceil() as usize) + 1;
            for i in 0..visible {
                let idx = first + i;
                if idx >= lines.len() {
                    break;
                }
                let ly = msg_y + 4.0 + i as f32 * line_h - (self.chat_msg_scroll % line_h);
                if ly < msg_y || ly > msg_y + msg_h {
                    continue;
                }
                let (ref text, color) = lines[idx];
                if text.is_empty() {
                    continue;
                }
                font::draw_fit(
                    text,
                    msg_x + 8.0,
                    ly,
                    1.05,
                    color.0,
                    color.1,
                    color.2,
                    color.3,
                    msg_w - 16.0,
                );
            }

            if msgs.is_empty() {
                let tm = theme::TEXT_MUTED;
                font::draw(
                    "No messages yet — say hi!",
                    msg_x + 8.0,
                    msg_y + msg_h * 0.45,
                    1.1,
                    tm.0,
                    tm.1,
                    tm.2,
                    tm.3,
                    0.0,
                );
            }
        }

        // Input + emoji + Send
        // Layout: [ input ........................ ] [😊] [Send]
        let input_x = msg_x;
        let emoji_btn_w = 36.0;
        let send_w = font::measure("Send", 1.05) + 20.0;
        let gap = 8.0;
        let input_w = (msg_w - send_w - emoji_btn_w - gap * 2.0).max(80.0);
        let focused = self.focus == Focus::ChatInput;
        gl::bevel_rect(input_x, input_y, input_w, input_h, theme::INPUT_BG, false);
        if focused {
            let g = theme::GOLD;
            gl::fill_rect(input_x, input_y + input_h - 1.0, input_w, 1.0, g.0, g.1, g.2, 0.9);
        }
        let ph = self.chat_peer_key.is_empty();
        let placeholder = self.chat_input.is_empty() && !focused;
        // Live-preview expands shortcodes for display only; buffer stays raw.
        let preview = if placeholder {
            String::new()
        } else {
            textutil::expand_shortcodes(&self.chat_input, &|n| emoji::from_shortcode(n))
        };
        let shown: &str = if placeholder {
            if ph {
                "Pick a conversation..."
            } else {
                "Message...  :fire:  or pick emoji"
            }
        } else {
            &preview
        };
        let color = if placeholder {
            theme::TEXT_MUTED
        } else {
            theme::TEXT
        };
        font::draw_fit(
            shown,
            input_x + 10.0,
            input_y + 7.0,
            1.1,
            color.0,
            color.1,
            color.2,
            color.3,
            input_w - 20.0,
        );
        // char counter
        let nchars = textutil::char_len(&self.chat_input);
        if nchars > 0 {
            let counter = format!("{nchars}/{}", textutil::MAX_CHAT_CHARS);
            let td = if nchars > textutil::MAX_CHAT_CHARS * 9 / 10 {
                theme::WARNING
            } else {
                theme::TEXT_MUTED
            };
            let cw = font::measure(&counter, 1.0);
            font::draw(
                &counter,
                input_x + input_w - cw - 6.0,
                input_y - 14.0,
                1.0,
                td.0,
                td.1,
                td.2,
                td.3,
                0.0,
            );
        }
        if focused
            && (std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_millis() / 500 % 2 == 0)
                .unwrap_or(false))
        {
            let caret_x =
                input_x + 10.0 + font::measure(shown, 1.1).min(input_w - 24.0);
            let tcol = theme::TEXT;
            gl::fill_rect(caret_x, input_y + 6.0, 2.0, 16.0, tcol.0, tcol.1, tcol.2, tcol.3);
        }
        if hit(mouse, input_x, input_y, input_w, input_h) && click {
            if !self.chat_peer_key.is_empty() {
                self.focus = Focus::ChatInput;
                self.emoji_picker = false;
            }
        }

        let emoji_x = input_x + input_w + gap;
        // Emoji toggle button
        {
            let hot = hit(mouse, emoji_x, input_y, emoji_btn_w, input_h);
            let face = if self.emoji_picker {
                theme::ACCENT
            } else if hot {
                theme::BTN_HOVER
            } else {
                theme::BTN
            };
            gl::bevel_rect(emoji_x, input_y, emoji_btn_w, input_h, face, !self.emoji_picker);
            // Prefer a real emoji glyph; fall back to ":)" label.
            if !emoji::draw('😊', emoji_x + 6.0, input_y + 2.0, 22.0) {
                let tcol = theme::TEXT;
                font::draw(":)", emoji_x + 8.0, input_y + 7.0, 1.1, tcol.0, tcol.1, tcol.2, tcol.3, 0.0);
            }
            if hot && click && !self.chat_peer_key.is_empty() {
                self.emoji_picker = !self.emoji_picker;
                self.focus = Focus::ChatInput;
            }
        }

        let send_x = emoji_x + emoji_btn_w + gap;
        if self.draw_btn("Send", send_x, input_y, input_h, true, mouse, click) {
            self.submit_chat();
        }

        // Emoji picker popover (above the input).
        if self.emoji_picker && !self.chat_peer_key.is_empty() {
            self.draw_emoji_picker(
                msg_x,
                input_y,
                msg_w.min(420.0),
                mouse,
                click,
                scroll_delta,
            );
        }
    }

    fn draw_emoji_picker(
        &mut self,
        anchor_x: f32,
        input_y: f32,
        width: f32,
        mouse: (f32, f32),
        click: bool,
        scroll_delta: f32,
    ) {
        let entries = emoji::picker_entries();
        if entries.is_empty() {
            return;
        }
        let cols = 10i32;
        let cell = 30.0f32;
        let pad = 8.0f32;
        let rows_visible = 5i32;
        let panel_w = pad * 2.0 + cols as f32 * cell;
        let panel_h = pad * 2.0 + rows_visible as f32 * cell + 22.0;
        let px = anchor_x;
        let py = input_y - panel_h - 6.0;

        gl::mc_panel(px, py, panel_w.min(width + 20.0).max(panel_w), panel_h, theme::PANEL_BG);
        let tcol = theme::TEXT_GOLD;
        font::draw(
            "Emoji  ·  click to insert  ·  or type :shortcode:",
            px + pad,
            py + 6.0,
            1.0,
            tcol.0,
            tcol.1,
            tcol.2,
            tcol.3,
            panel_w - pad * 2.0,
        );

        let grid_y = py + 24.0;
        let grid_h = rows_visible as f32 * cell;
        let total_rows = ((entries.len() as i32) + cols - 1) / cols;
        let max_scroll = ((total_rows - rows_visible).max(0) as f32) * cell;
        if hit(mouse, px, grid_y, panel_w, grid_h) {
            self.emoji_picker_scroll =
                (self.emoji_picker_scroll - scroll_delta * cell).clamp(0.0, max_scroll);
        }
        // Close button
        if self.draw_btn("X", px + panel_w - 28.0, py + 4.0, 18.0, false, mouse, click) {
            self.emoji_picker = false;
            return;
        }

        let start_row = (self.emoji_picker_scroll / cell).floor() as i32;
        let mut picked: Option<char> = None;
        for row in 0..rows_visible {
            let r = start_row + row;
            if r >= total_rows {
                break;
            }
            for c in 0..cols {
                let idx = (r * cols + c) as usize;
                if idx >= entries.len() {
                    break;
                }
                let (_code, ch) = &entries[idx];
                let cx = px + pad + c as f32 * cell;
                let cy = grid_y + row as f32 * cell;
                let hot = hit(mouse, cx, cy, cell, cell);
                if hot {
                    gl::bevel_rect(cx + 1.0, cy + 1.0, cell - 2.0, cell - 2.0, theme::BTN_HOVER, true);
                }
                let _ = emoji::draw(*ch, cx + 3.0, cy + 3.0, 24.0);
                if hot && click {
                    picked = Some(*ch);
                }
            }
        }
        if let Some(ch) = picked {
            if textutil::char_len(&self.chat_input) < textutil::MAX_CHAT_CHARS {
                self.chat_input.push(ch);
            }
            self.focus = Focus::ChatInput;
            // keep picker open for multi-insert
        }
        // Click outside closes
        if click && !hit(mouse, px, py, panel_w, panel_h)
            && !hit(mouse, px, input_y, panel_w, 32.0)
        {
            // only if click wasn't on emoji button — handled by toggle above order
        }
    }

    fn submit_chat(&mut self) {
        if self.chat_peer_key.is_empty() {
            return;
        }
        if !textutil::is_safe_key(&self.chat_peer_key) {
            self.status = "Invalid peer key".into();
            return;
        }
        // Expand :shortcodes: then strip controls / bidi / oversize.
        let text = textutil::prepare_outgoing(&self.chat_input, &|n| emoji::from_shortcode(n));
        if text.is_empty() {
            return;
        }
        if !textutil::within_chat_limits(&text) {
            self.status = "Message too long".into();
            return;
        }
        let peer = self.chat_peer_key.clone();
        match ipc::send_chat(&peer, &text) {
            Ok(v) => {
                if let Some(err) = v.get("error").and_then(|e| e.as_str()) {
                    self.status = format!("Send failed: {err}");
                } else {
                    self.chat_input.clear();
                    self.emoji_picker = false;
                    let id = v.get("id").and_then(|i| i.as_i64()).unwrap_or(0);
                    let safe_body = textutil::prepare_inbound(&text);
                    self.messages.push(ipc::ChatMessage {
                        id,
                        conversation: String::new(),
                        from_key: self.session.uuid.clone(),
                        from_name: textutil::sanitize_chat(&self.session.username),
                        to_key: peer,
                        body: safe_body,
                        at: String::new(),
                    });
                    if id > self.chat_cursor {
                        self.chat_cursor = id;
                    }
                    self.chat_msg_scroll = -1.0;
                    self.status = "Message sent".into();
                    self.poll_chat(true);
                }
            }
            Err(e) => self.status = format!("Send failed: {e}"),
        }
    }

    // ── Widgets ───────────────────────────────────────────────────────

    fn draw_btn(
        &self,
        label: &str,
        x: f32,
        y: f32,
        h: f32,
        accent: bool,
        mouse: (f32, f32),
        click: bool,
    ) -> bool {
        let w = font::measure(label, 1.05) + 20.0;
        let hot = hit(mouse, x, y, w, h);
        let face = if accent {
            if hot {
                theme::ACCENT_HOVER
            } else {
                theme::ACCENT
            }
        } else if hot {
            theme::BTN_HOVER
        } else {
            theme::BTN
        };
        // Pressed look while held would need mouse-down state; click-edge uses raised.
        gl::bevel_rect(x, y, w, h, face, true);
        let t = theme::TEXT;
        let tw = font::measure(label, 1.05);
        font::draw(
            label,
            x + (w - tw) * 0.5,
            y + (h - 14.0) * 0.5,
            1.05,
            t.0,
            t.1,
            t.2,
            t.3,
            0.0,
        );
        hot && click
    }

    /// Draw a compact row action button. Returns (clicked, new_left_edge).
    fn row_btn_hit(
        &self,
        label: &str,
        right_edge: f32,
        y: f32,
        mouse: (f32, f32),
        click: bool,
    ) -> (bool, f32) {
        let w = font::measure(label, 1.0) + 14.0;
        let h = 20.0;
        let x = right_edge - w;
        let hot = hit(mouse, x, y, w, h);
        let use_bg = if label == "OK" || label == "Chat" {
            if hot {
                theme::ACCENT_HOVER
            } else {
                theme::ACCENT
            }
        } else if hot {
            theme::BTN_HOVER
        } else {
            theme::BTN
        };
        gl::bevel_rect(x, y, w, h, use_bg, true);
        let t = theme::TEXT;
        font::draw(label, x + 7.0, y + 6.0, 1.0, t.0, t.1, t.2, t.3, 0.0);
        (hot && click, x - 6.0)
    }

    // ── Data ──────────────────────────────────────────────────────────

    fn refresh_data(&mut self, force: bool) {
        if !force && self.last_fetch.elapsed() < std::time::Duration::from_secs(5) {
            return;
        }
        self.last_fetch = std::time::Instant::now();
        self.session = ipc::fetch_session();
        self.feed = ipc::fetch_youtube_feed();
        let mut friends = ipc::fetch_friends();
        ipc::apply_presence(&mut friends);
        self.friends = friends;
        self.status = format!(
            "Feed {} · Friends {} · {}",
            self.feed.len(),
            self.friends.friends.len(),
            if self.session.username.is_empty() {
                "no session"
            } else {
                &self.session.username
            }
        );
    }

    fn poll_chat(&mut self, force: bool) {
        if !force && self.last_chat_poll.elapsed() < std::time::Duration::from_secs(3) {
            return;
        }
        self.last_chat_poll = std::time::Instant::now();
        let batch = ipc::fetch_chat(self.chat_cursor);
        if !batch.ok && !batch.error.is_empty() {
            return;
        }
        let my_key = self.session.uuid.clone();
        for m in batch.messages {
            if self.messages.iter().any(|e| e.id == m.id) {
                continue;
            }
            // Unread when the message is for me and we're not currently viewing that peer.
            let viewing = self.tab == Tab::Chat && m.from_key == self.chat_peer_key;
            if m.to_key == my_key && !viewing && !m.from_key.is_empty() {
                *self.unread.entry(m.from_key.clone()).or_insert(0) += 1;
            }
            let mut m = m;
            m.body = textutil::prepare_inbound(&m.body);
            m.from_name = textutil::sanitize_chat(&m.from_name);
            // Drop messages with empty body after sanitise (pure control chars).
            if m.body.is_empty() {
                continue;
            }
            self.messages.push(m);
        }
        if batch.cursor > self.chat_cursor {
            self.chat_cursor = batch.cursor;
        }
        // Cap history
        if self.messages.len() > 2000 {
            let drain = self.messages.len() - 1500;
            self.messages.drain(0..drain);
        }
    }

    fn drain_text_input(&mut self) {
        let chars: Vec<char> = self.char_queue.drain(..).collect();
        for ch in chars {
            match self.focus {
                Focus::FriendAdd => {
                    if self.friend_add.len() < 32 && !ch.is_control() {
                        self.friend_add.push(ch);
                    }
                }
                Focus::ChatInput => {
                    if ch.is_control() {
                        continue;
                    }
                    // Soft-cap raw buffer a bit over the sanitized limit so
                    // users can type shortcodes before expansion.
                    if textutil::char_len(&self.chat_input) < textutil::MAX_CHAT_CHARS
                        && self.chat_input.len() < textutil::MAX_CHAT_BYTES
                    {
                        self.chat_input.push(ch);
                    }
                }
                Focus::YtSearch => {
                    if self.yt_query.len() < 256 && !ch.is_control() {
                        self.yt_query.push(ch);
                        self.yt_scroll = 0.0;
                    }
                }
                Focus::None => {}
            }
        }
    }

    fn handle_edit_keys(&mut self) {
        if self.focus == Focus::None {
            return;
        }
        if key_edge(VK_BACK.0 as i32) {
            match self.focus {
                Focus::FriendAdd => {
                    self.friend_add.pop();
                }
                Focus::ChatInput => {
                    self.chat_input.pop();
                }
                Focus::YtSearch => {
                    self.yt_query.pop();
                    self.yt_scroll = 0.0;
                }
                Focus::None => {}
            }
        }
        if key_edge(VK_DELETE.0 as i32) {
            match self.focus {
                Focus::FriendAdd => self.friend_add.clear(),
                Focus::ChatInput => self.chat_input.clear(),
                Focus::YtSearch => {
                    self.yt_query.clear();
                    self.yt_scroll = 0.0;
                }
                Focus::None => {}
            }
        }
        if key_edge(VK_RETURN.0 as i32) {
            match self.focus {
                Focus::FriendAdd => self.submit_add_friend(),
                Focus::ChatInput => self.submit_chat(),
                Focus::YtSearch => self.play_from_query(),
                Focus::None => {}
            }
        }
    }
}

enum FriendAction {
    Accept(i64),
    Remove(i64),
    OpenChat { key: String, name: String },
}

fn conversation_matches(conv: &str, a: &str, b: &str) -> bool {
    if conv.is_empty() || a.is_empty() || b.is_empty() {
        return false;
    }
    let (x, y) = if a < b { (a, b) } else { (b, a) };
    conv == format!("{x}:{y}")
}

fn fmt_clock(secs: f64) -> String {
    let total = secs.max(0.0) as u64;
    let h = total / 3600;
    let m = (total % 3600) / 60;
    let s = total % 60;
    if h > 0 {
        format!("{h}:{m:02}:{s:02}")
    } else {
        format!("{m}:{s:02}")
    }
}

/// Pull an 11-char YouTube video id out of common URL / bare-id forms.
fn extract_youtube_id(raw: &str) -> Option<String> {
    let s = raw.trim();
    // Bare id
    if s.len() == 11
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        return Some(s.to_string());
    }
    // watch?v=ID
    if let Some(idx) = s.find("v=") {
        let rest = &s[idx + 2..];
        let id: String = rest
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
            .collect();
        if id.len() == 11 {
            return Some(id);
        }
    }
    // youtu.be/ID
    if let Some(idx) = s.find("youtu.be/") {
        let rest = &s[idx + "youtu.be/".len()..];
        let id: String = rest
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
            .collect();
        if id.len() == 11 {
            return Some(id);
        }
    }
    // /embed/ID or /shorts/ID
    for marker in ["/embed/", "/shorts/"] {
        if let Some(idx) = s.find(marker) {
            let rest = &s[idx + marker.len()..];
            let id: String = rest
                .chars()
                .take_while(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
                .collect();
            if id.len() == 11 {
                return Some(id);
            }
        }
    }
    None
}

fn cursor() -> (f32, f32) {
    unsafe {
        let mut pt = POINT { x: 0, y: 0 };
        let _ = GetCursorPos(&mut pt);
        gl::screen_to_overlay(pt.x, pt.y)
    }
}

fn left_click_edge() -> bool {
    static WAS: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
    let down = unsafe { GetAsyncKeyState(VK_LBUTTON.0 as i32) } < 0;
    let was = WAS.load(std::sync::atomic::Ordering::SeqCst);
    WAS.store(down, std::sync::atomic::Ordering::SeqCst);
    down && !was
}

fn key_edge(vk: i32) -> bool {
    use std::sync::atomic::{AtomicBool, Ordering};
    // Dedicated edge detectors for the few edit keys we care about.
    static BACK_WAS: AtomicBool = AtomicBool::new(false);
    static DEL_WAS: AtomicBool = AtomicBool::new(false);
    static RET_WAS: AtomicBool = AtomicBool::new(false);
    let cell = if vk == VK_BACK.0 as i32 {
        &BACK_WAS
    } else if vk == VK_DELETE.0 as i32 {
        &DEL_WAS
    } else if vk == VK_RETURN.0 as i32 {
        &RET_WAS
    } else {
        return false;
    };
    let down = unsafe { GetAsyncKeyState(vk) } < 0;
    let was = cell.load(Ordering::SeqCst);
    cell.store(down, Ordering::SeqCst);
    down && !was
}

/// Keyboard scroll: Up/Down/PgUp/PgDn while overlay is open.
fn scroll_keys() -> f32 {
    let mut d = 0.0f32;
    unsafe {
        if GetAsyncKeyState(VK_DOWN.0 as i32) < 0 {
            d -= 0.6;
        }
        if GetAsyncKeyState(VK_UP.0 as i32) < 0 {
            d += 0.6;
        }
        if GetAsyncKeyState(VK_NEXT.0 as i32) < 0 {
            d -= 2.5;
        }
        if GetAsyncKeyState(VK_PRIOR.0 as i32) < 0 {
            d += 2.5;
        }
    }
    d
}

fn hit(mouse: (f32, f32), x: f32, y: f32, w: f32, h: f32) -> bool {
    mouse.0 >= x && mouse.0 <= x + w && mouse.1 >= y && mouse.1 <= y + h
}
