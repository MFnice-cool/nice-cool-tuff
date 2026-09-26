import { writable } from "svelte/store";
import { api } from "./api";

/**
 * Personal YouTube feed (docs: home feed sources): the player chooses the
 * channels that form the home strip — their YouTube, not the curated table.
 *
 * Channel list + enabled flag persist in localStorage (same pattern as the
 * feed's own expanded/fullView flags). Videos are fetched Rust-side
 * (`youtube_my_feed_fetch`): the RSS hub sends no CORS headers, so the
 * webview cannot reach it directly.
 */

export type MyFeedChannel = {
  /** YouTube channel id (UC…). */
  id: string;
  /** Channel title at the time it was added. */
  label: string;
};

export type MyFeedConfig = {
  enabled: boolean;
  channels: MyFeedChannel[];
};

const STORAGE_KEY = "tuffbox-youtube-my-feed";

function load(): MyFeedConfig {
  try {
    const raw = localStorage.getItem(STORAGE_KEY);
    if (raw) {
      const parsed = JSON.parse(raw) as Partial<MyFeedConfig>;
      return {
        enabled: !!parsed.enabled,
        channels: Array.isArray(parsed.channels) ? parsed.channels : [],
      };
    }
  } catch {
    // ignore storage errors
  }
  return { enabled: false, channels: [] };
}

export const youtubeMyFeed = writable<MyFeedConfig>(load());

youtubeMyFeed.subscribe((cfg) => {
  try {
    localStorage.setItem(STORAGE_KEY, JSON.stringify(cfg));
  } catch {
    // ignore storage errors
  }
});

/** Resolve a pasted channel reference (@handle / URL / UC id) and add it. */
export async function addMyFeedChannel(
  query: string,
): Promise<{ ok: boolean; error?: string }> {
  const q = query.trim();
  if (!q) return { ok: false, error: "Paste a channel link, @handle, or channel ID." };
  let resolved: { channelId: string; label: string };
  try {
    resolved = await api.youtubeMyFeed.lookup(q);
  } catch (e) {
    return { ok: false, error: String(e) };
  }
  let duplicate = false;
  youtubeMyFeed.update((cfg) => {
    if (cfg.channels.some((c) => c.id === resolved.channelId)) {
      duplicate = true;
      return cfg;
    }
    return {
      ...cfg,
      enabled: true,
      channels: [...cfg.channels, { id: resolved.channelId, label: resolved.label }],
    };
  });
  if (duplicate) return { ok: false, error: "That channel is already in your feed." };
  return { ok: true };
}

export function removeMyFeedChannel(id: string) {
  youtubeMyFeed.update((cfg) => ({
    ...cfg,
    channels: cfg.channels.filter((c) => c.id !== id),
  }));
}

export function setMyFeedEnabled(enabled: boolean) {
  youtubeMyFeed.update((cfg) => ({ ...cfg, enabled }));
}

/** Latest videos from the player's channels, newest first. */
export async function fetchMyFeedVideos(): Promise<
  {
    video_id: string;
    title: string;
    thumbnail_url: string | null;
    channel_name: string | null;
    source: string;
    view_count: number | null;
    published_at: string | null;
  }[]
> {
  let ids: string[] = [];
  const unsubscribe = youtubeMyFeed.subscribe((cfg) => {
    ids = cfg.channels.map((c) => c.id);
  });
  unsubscribe();
  if (ids.length === 0) return [];
  const res = await api.youtubeMyFeed.fetch(ids.slice(0, 24));
  return res.videos
    .map((v) => ({
      video_id: v.videoId,
      title: v.title,
      thumbnail_url: v.thumbnailUrl ?? null,
      channel_name: v.channelName ?? null,
      source: "my",
      view_count: v.viewCount ?? null,
      published_at: v.publishedAt ?? null,
    }))
    .sort((a, b) => (a.published_at ?? "").localeCompare(b.published_at ?? ""))
    .reverse();
}
