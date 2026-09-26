// Crawl popular Minecraft YouTube videos → youtube_feed (service role).
// Popular keyword hits first (per locale), then tracked creator channels.
// Clients filter by user language OR English.

import { createClient } from "https://esm.sh/@supabase/supabase-js@2.49.1";

type Duration = "any" | "medium" | "long";

type LocaleCrawl = {
  lang: string;
  region: string;
  queries: Array<{ q: string; duration?: Duration }>;
};

/** Popular searches per locale (relevanceLanguage + regionCode). */
const LOCALES: LocaleCrawl[] = [
  {
    lang: "en",
    region: "US",
    queries: [
      { q: "Minecraft mods", duration: "medium" },
      { q: "Minecraft 1.21 survival", duration: "medium" },
      { q: "Minecraft modpack", duration: "medium" },
      { q: "Minecraft redstone", duration: "medium" },
      { q: "Minecraft hardcore", duration: "medium" },
      { q: "Minecraft build tutorial", duration: "medium" },
    ],
  },
  {
    lang: "ru",
    region: "RU",
    queries: [
      { q: "Майнкрафт моды", duration: "medium" },
      { q: "Minecraft 1.21 выживание", duration: "medium" },
      { q: "Майнкрафт модпак", duration: "medium" },
      { q: "Майнкрафт редстоун", duration: "medium" },
      { q: "Майнкрафт хардкор", duration: "medium" },
      { q: "Майнкрафт постройки", duration: "medium" },
    ],
  },
  {
    lang: "uk",
    region: "UA",
    queries: [
      { q: "Майнкрафт моди", duration: "medium" },
      { q: "Minecraft виживання", duration: "medium" },
      { q: "популярний Minecraft", duration: "medium" },
    ],
  },
  {
    lang: "de",
    region: "DE",
    queries: [
      { q: "Minecraft Mods", duration: "medium" },
      { q: "Minecraft Survival", duration: "medium" },
      { q: "Minecraft Modpack", duration: "medium" },
    ],
  },
  {
    lang: "es",
    region: "ES",
    queries: [
      { q: "Minecraft mods", duration: "medium" },
      { q: "Minecraft supervivencia", duration: "medium" },
      { q: "Minecraft modpack", duration: "medium" },
    ],
  },
  {
    lang: "fr",
    region: "FR",
    queries: [
      { q: "Minecraft mods", duration: "medium" },
      { q: "Minecraft survie", duration: "medium" },
      { q: "Minecraft modpack", duration: "medium" },
    ],
  },
  {
    lang: "pt",
    region: "BR",
    queries: [
      { q: "Minecraft mods", duration: "medium" },
      { q: "Minecraft sobrevivência", duration: "medium" },
      { q: "Minecraft modpack", duration: "medium" },
    ],
  },
  {
    lang: "pl",
    region: "PL",
    queries: [
      { q: "Minecraft mody", duration: "medium" },
      { q: "Minecraft survival", duration: "medium" },
      { q: "Minecraft modpack", duration: "medium" },
    ],
  },
];

/** Creator channels to track (resolved via search type=channel each run). */
const TRACKED_CHANNELS = [
  "Dr Donut",
  "Dream",
  "Carvs",
  "Kaisora",
  "JudeLow",
] as const;

const SEARCH_MAX = 6;
const CHANNEL_UPLOADS = 6;
/** Cap popular rows kept per language (after diverse pick). */
const POPULAR_PER_LANG = 14;
/** Cap channel rows kept overall. */
const CHANNEL_TOP = 24;
/** Max popular clips kept from one channel per language. */
const MAX_PER_CHANNEL_POPULAR = 2;
const LOOKBACK_DAYS = 14;

const corsHeaders: Record<string, string> = {
  "Access-Control-Allow-Origin": "*",
  "Access-Control-Allow-Headers":
    "authorization, x-client-info, apikey, content-type",
};

type FeedRow = {
  video_id: string;
  title: string;
  thumbnail_url: string;
  channel_name: string;
  published_at: string | null;
  view_count: number;
  fetched_at: string;
  query_tag: string;
  source: "popular" | "channel";
  lang: string;
};

type Candidate = {
  videoId: string;
  queryTag: string;
  source: "popular" | "channel";
  lang: string;
};

function jsonResponse(status: number, body: unknown): Response {
  return new Response(JSON.stringify(body), {
    status,
    headers: { ...corsHeaders, "Content-Type": "application/json" },
  });
}

function publishedAfterIso(days: number): string {
  const d = new Date();
  d.setUTCDate(d.getUTCDate() - days);
  return d.toISOString();
}

/** Prefer high views but cap per channel so one creator can't own the pool. */
function pickDiverseByChannel(
  rows: FeedRow[],
  limit: number,
  maxPerChannel: number,
): FeedRow[] {
  const sorted = [...rows].sort((a, b) => b.view_count - a.view_count);
  const out: FeedRow[] = [];
  const counts = new Map<string, number>();
  for (const row of sorted) {
    if (out.length >= limit) break;
    const key = row.channel_name.trim().toLowerCase() || "?";
    const n = counts.get(key) ?? 0;
    if (n >= maxPerChannel) continue;
    counts.set(key, n + 1);
    out.push(row);
  }
  if (out.length < limit) {
    for (const row of sorted) {
      if (out.length >= limit) break;
      if (out.some((r) => r.video_id === row.video_id)) continue;
      out.push(row);
    }
  }
  return out;
}

/** Round-robin across creators so Dream/etc. don't fill CHANNEL_TOP alone. */
function pickChannelRoundRobin(rows: FeedRow[], limit: number): FeedRow[] {
  const byChannel = new Map<string, FeedRow[]>();
  for (const row of rows) {
    const key = row.channel_name.trim().toLowerCase() || "?";
    const list = byChannel.get(key) ?? [];
    list.push(row);
    byChannel.set(key, list);
  }
  for (const list of byChannel.values()) {
    list.sort((a, b) => b.view_count - a.view_count);
  }
  const out: FeedRow[] = [];
  let round = 0;
  while (out.length < limit) {
    let added = false;
    for (const list of byChannel.values()) {
      if (round < list.length && out.length < limit) {
        out.push(list[round]);
        added = true;
      }
    }
    if (!added) break;
    round++;
  }
  return out;
}

function pickThumb(snippet: Record<string, unknown>): string {
  const thumbs = snippet.thumbnails as
    | Record<string, { url?: string } | undefined>
    | undefined;
  return (
    thumbs?.medium?.url ||
    thumbs?.high?.url ||
    thumbs?.default?.url ||
    ""
  );
}

/** Normalize YouTube language tags (en-US → en). */
function primaryLang(raw: unknown): string | null {
  if (typeof raw !== "string") return null;
  const t = raw.trim().toLowerCase();
  if (!t) return null;
  const primary = t.split(/[-_]/)[0];
  return primary && primary.length >= 2 ? primary.slice(0, 8) : null;
}

/** Cheap script heuristic when YouTube omits defaultAudioLanguage. */
function guessLangFromTitle(title: string): string | null {
  if (/[\u0400-\u04FF]/.test(title)) {
    // Ukrainian letters often present; otherwise Russian
    if (/[іїєґІЇЄҐ]/.test(title)) return "uk";
    return "ru";
  }
  if (/[\u3040-\u30ff\u31f0-\u31ff]/.test(title)) return "ja";
  if (/[\uac00-\ud7af]/.test(title)) return "ko";
  if (/[\u4e00-\u9fff]/.test(title)) return "zh";
  if (/[äöüß]/i.test(title) && /\b(mod|minecraft|überleben|welt)\b/i.test(title)) {
    return "de";
  }
  return null;
}

function resolveVideoLang(
  snippet: Record<string, unknown>,
  fallback: string,
): string {
  return (
    primaryLang(snippet.defaultAudioLanguage) ||
    primaryLang(snippet.defaultLanguage) ||
    guessLangFromTitle(String(snippet.title ?? "")) ||
    fallback
  );
}

async function ytFetch(url: string, attempts = 3): Promise<Response> {
  let lastErr: Error | null = null;
  for (let i = 0; i < attempts; i++) {
    try {
      const res = await fetch(url);
      if (res.status === 429 || res.status >= 500) {
        lastErr = new Error(`YouTube HTTP ${res.status}`);
        await new Promise((r) => setTimeout(r, 350 * (i + 1) * (i + 1)));
        continue;
      }
      return res;
    } catch (e) {
      lastErr = e instanceof Error ? e : new Error(String(e));
      await new Promise((r) => setTimeout(r, 350 * (i + 1) * (i + 1)));
    }
  }
  throw lastErr ?? new Error("YouTube fetch failed");
}

async function searchVideoIds(
  apiKey: string,
  query: string,
  opts: {
    duration?: Duration;
    channelId?: string;
    order?: "viewCount" | "date";
    publishedAfterDays?: number;
    maxResults?: number;
    relevanceLanguage?: string;
    regionCode?: string;
  } = {},
): Promise<string[]> {
  const params = new URLSearchParams({
    part: "snippet",
    type: "video",
    order: opts.order ?? "viewCount",
    maxResults: String(opts.maxResults ?? SEARCH_MAX),
    key: apiKey,
  });
  if (opts.channelId) {
    params.set("channelId", opts.channelId);
  } else {
    params.set("q", query);
  }
  if (opts.relevanceLanguage) {
    params.set("relevanceLanguage", opts.relevanceLanguage);
  }
  if (opts.regionCode) {
    params.set("regionCode", opts.regionCode);
  }
  const days = opts.publishedAfterDays ?? LOOKBACK_DAYS;
  if (days > 0) {
    params.set("publishedAfter", publishedAfterIso(days));
  }
  if (opts.duration && opts.duration !== "any") {
    params.set("videoDuration", opts.duration);
  }
  const res = await ytFetch(
    `https://www.googleapis.com/youtube/v3/search?${params}`,
  );
  if (!res.ok) {
    const err = await res.text();
    throw new Error(`YouTube search failed (${res.status}): ${err.slice(0, 200)}`);
  }
  const data = await res.json();
  const items = Array.isArray(data.items) ? data.items : [];
  const ids: string[] = [];
  for (const item of items) {
    const id = item?.id?.videoId;
    if (typeof id === "string" && id.trim()) ids.push(id.trim());
  }
  return ids;
}

async function resolveChannelId(
  apiKey: string,
  name: string,
): Promise<string | null> {
  const params = new URLSearchParams({
    part: "snippet",
    type: "channel",
    q: name,
    maxResults: "1",
    key: apiKey,
  });
  const res = await ytFetch(
    `https://www.googleapis.com/youtube/v3/search?${params}`,
  );
  if (!res.ok) return null;
  const data = await res.json();
  const id = data?.items?.[0]?.id?.channelId;
  return typeof id === "string" && id.trim() ? id.trim() : null;
}

async function fetchVideoDetails(
  apiKey: string,
  ids: string[],
): Promise<Map<string, Record<string, unknown>>> {
  const out = new Map<string, Record<string, unknown>>();
  if (ids.length === 0) return out;
  for (let i = 0; i < ids.length; i += 50) {
    const chunk = ids.slice(i, i + 50);
    const params = new URLSearchParams({
      part: "snippet,statistics",
      id: chunk.join(","),
      key: apiKey,
    });
    const res = await ytFetch(
      `https://www.googleapis.com/youtube/v3/videos?${params}`,
    );
    if (!res.ok) {
      const err = await res.text();
      throw new Error(`YouTube videos failed (${res.status}): ${err.slice(0, 200)}`);
    }
    const data = await res.json();
    const items = Array.isArray(data.items) ? data.items : [];
    for (const item of items) {
      if (typeof item?.id === "string") out.set(item.id, item);
    }
  }
  return out;
}

function toRow(
  videoId: string,
  item: Record<string, unknown>,
  cand: Candidate,
  now: string,
): FeedRow | null {
  const snippet = (item.snippet ?? {}) as Record<string, unknown>;
  const stats = (item.statistics ?? {}) as Record<string, unknown>;
  const title = String(snippet.title ?? "").trim();
  const channel = String(snippet.channelTitle ?? "").trim();
  const thumb = pickThumb(snippet).trim();
  if (!title || !channel || !thumb) return null;
  const viewCount = Number.parseInt(String(stats.viewCount ?? "0"), 10) || 0;
  const published =
    typeof snippet.publishedAt === "string" ? snippet.publishedAt : null;
  const lang =
    cand.source === "popular"
      ? cand.lang
      : resolveVideoLang(snippet, cand.lang);
  return {
    video_id: videoId,
    title,
    thumbnail_url: thumb,
    channel_name: channel,
    published_at: published,
    view_count: viewCount,
    fetched_at: now,
    query_tag: cand.queryTag,
    source: cand.source,
    lang,
  };
}

Deno.serve(async (req) => {
  if (req.method === "OPTIONS") {
    return new Response("ok", { headers: corsHeaders });
  }
  if (req.method !== "POST" && req.method !== "GET") {
    return jsonResponse(405, { error: "method not allowed" });
  }

  const apiKey = (
    Deno.env.get("YOUTUBE-API-KEY") ??
    Deno.env.get("YOUTUBE_API_KEY") ??
    ""
  ).trim();
  const supabaseUrl = Deno.env.get("SUPABASE_URL") ?? "";
  const serviceKey = Deno.env.get("SUPABASE_SERVICE_ROLE_KEY") ?? "";
  if (!apiKey) {
    return jsonResponse(500, {
      error: "YOUTUBE-API-KEY (or YOUTUBE_API_KEY) not configured",
    });
  }
  if (!supabaseUrl || !serviceKey) {
    return jsonResponse(500, { error: "server misconfigured" });
  }

  const admin = createClient(supabaseUrl, serviceKey, {
    auth: { persistSession: false, autoRefreshToken: false },
  });

  try {
    // videoId → first win (popular before channel so keyword hits keep popular source)
    const candidates = new Map<string, Candidate>();

    // 1) Popular per locale
    for (const locale of LOCALES) {
      for (const item of locale.queries) {
        const ids = await searchVideoIds(apiKey, item.q, {
          duration: item.duration,
          order: "viewCount",
          publishedAfterDays: LOOKBACK_DAYS,
          relevanceLanguage: locale.lang,
          regionCode: locale.region,
        });
        for (const id of ids) {
          if (candidates.has(id)) continue;
          candidates.set(id, {
            videoId: id,
            queryTag: item.q,
            source: "popular",
            lang: locale.lang,
          });
        }
      }
    }

    // 2) Tracked creators (after popular)
    const seenChannels = new Set<string>();
    for (const name of TRACKED_CHANNELS) {
      const key = name.toLowerCase();
      if (seenChannels.has(key)) continue;
      seenChannels.add(key);
      const channelId = await resolveChannelId(apiKey, name);
      if (!channelId) continue;
      const recent = await searchVideoIds(apiKey, name, {
        channelId,
        order: "date",
        duration: "medium",
        publishedAfterDays: 60,
        maxResults: CHANNEL_UPLOADS,
      });
      const popular = await searchVideoIds(apiKey, name, {
        channelId,
        order: "viewCount",
        duration: "any",
        publishedAfterDays: 90,
        maxResults: CHANNEL_UPLOADS,
      });
      for (const id of [...recent, ...popular]) {
        if (candidates.has(id)) continue;
        candidates.set(id, {
          videoId: id,
          queryTag: `channel:${name}`,
          source: "channel",
          lang: "en", // refined from video metadata below
        });
      }
    }

    const allIds = [...candidates.keys()];
    const details = await fetchVideoDetails(apiKey, allIds);
    const now = new Date().toISOString();

    const popularByLang = new Map<string, FeedRow[]>();
    const channelRows: FeedRow[] = [];

    for (const [videoId, cand] of candidates) {
      const item = details.get(videoId);
      if (!item) continue;
      const row = toRow(videoId, item, cand, now);
      if (!row) continue;
      if (row.source === "popular") {
        const list = popularByLang.get(row.lang) ?? [];
        list.push(row);
        popularByLang.set(row.lang, list);
      } else {
        channelRows.push(row);
      }
    }

    const stored: FeedRow[] = [];
    for (const [, list] of popularByLang) {
      stored.push(...pickDiverseByChannel(list, POPULAR_PER_LANG, MAX_PER_CHANNEL_POPULAR));
    }
    stored.push(...pickChannelRoundRobin(channelRows, CHANNEL_TOP));

    // Deduplicate by video_id (same clip can win popular+channel race across langs)
    const dedup = new Map<string, FeedRow>();
    for (const row of stored) {
      const prev = dedup.get(row.video_id);
      if (!prev) {
        dedup.set(row.video_id, row);
        continue;
      }
      // Prefer popular over channel; else higher views
      if (prev.source === "channel" && row.source === "popular") {
        dedup.set(row.video_id, row);
      } else if (prev.source === row.source && row.view_count > prev.view_count) {
        dedup.set(row.video_id, row);
      }
    }
    const top = [...dedup.values()];

    // Never wipe-then-insert: upsert first; only prune orphans after a non-empty batch.
    if (top.length === 0) {
      return jsonResponse(200, {
        ok: true,
        candidates: allIds.length,
        stored: 0,
        skippedWipe: true,
        message: "empty crawl — left existing youtube_feed rows intact",
      });
    }

    const { error: upsertErr } = await admin
      .from("youtube_feed")
      .upsert(top, { onConflict: "video_id" });
    if (upsertErr) {
      return jsonResponse(500, { error: `upsert failed: ${upsertErr.message}` });
    }

    const keepIds = top.map((r) => r.video_id);
    const { error: delErr } = await admin
      .from("youtube_feed")
      .delete()
      .not("video_id", "in", `(${keepIds.join(",")})`);
    if (delErr) {
      // Non-fatal: feed still has fresh rows; orphans may linger until next success.
      console.error("youtube_feed prune failed:", delErr.message);
    }

    const byLang: Record<string, number> = {};
    let popularCount = 0;
    let channelCount = 0;
    for (const row of top) {
      byLang[row.lang] = (byLang[row.lang] ?? 0) + 1;
      if (row.source === "popular") popularCount++;
      else channelCount++;
    }

    return jsonResponse(200, {
      ok: true,
      candidates: allIds.length,
      stored: top.length,
      popular: popularCount,
      channel: channelCount,
      byLang,
    });
  } catch (e) {
    const msg = e instanceof Error ? e.message : String(e);
    console.error("fetch-youtube-feed", msg);
    return jsonResponse(500, { error: msg });
  }
});
