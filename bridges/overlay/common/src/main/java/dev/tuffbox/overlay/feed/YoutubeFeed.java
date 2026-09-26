package dev.tuffbox.overlay.feed;

import dev.tuffbox.overlay.core.OverlayCore;

import java.util.ArrayList;
import java.util.List;
import java.util.Locale;

/**
 * Reads the shared youtube_feed table (populated by the fetch-youtube-feed
 * edge function for the desktop home feed) via PostgREST — no YouTube API
 * key needed. Fetches a pool once, filters locally for instant search.
 */
public final class YoutubeFeed {

    public static final class FeedVideo {
        public String videoId = "";
        public String title = "";
        public String channel = "";
        public long views;
        public String lang = "";

        public String watchUrl() {
            return "https://www.youtube.com/watch?v=" + videoId;
        }
    }

    private YoutubeFeed() {}

    /** Fetch the popular pool (EN+RU), ordered by views. Null session → empty. */
    public static List<FeedVideo> fetchPool(OverlayCore.Session session, int limit) {
        List<FeedVideo> out = new ArrayList<FeedVideo>();
        if (session == null) {
            return out;
        }
        String q = "youtube_feed?select=video_id,title,channel_name,view_count,lang"
                + "&lang=in.(en,ru,uk)"
                + "&order=view_count.desc"
                + "&limit=" + Math.max(1, Math.min(limit, 200));
        String body = OverlayCore.httpGetRest(session, q);
        if (body == null || body.isEmpty()) {
            return out;
        }
        for (String obj : OverlayCore.splitObjects(body)) {
            FeedVideo v = new FeedVideo();
            v.videoId = nullToEmpty(OverlayCore.jsonString(obj, "video_id"));
            v.title = nullToEmpty(OverlayCore.jsonString(obj, "title"));
            v.channel = nullToEmpty(OverlayCore.jsonString(obj, "channel_name"));
            v.views = OverlayCore.jsonLong(obj, "view_count", 0);
            v.lang = nullToEmpty(OverlayCore.jsonString(obj, "lang"));
            if (!v.videoId.isEmpty() && !v.title.isEmpty()) {
                out.add(v);
            }
        }
        return out;
    }

    /** Case-insensitive substring filter over title + channel. */
    public static List<FeedVideo> filter(List<FeedVideo> pool, String query) {
        List<FeedVideo> out = new ArrayList<FeedVideo>();
        if (query == null || query.trim().isEmpty()) {
            out.addAll(pool);
            return out;
        }
        String needle = query.trim().toLowerCase(Locale.ROOT);
        for (FeedVideo v : pool) {
            if (v.title.toLowerCase(Locale.ROOT).contains(needle)
                    || v.channel.toLowerCase(Locale.ROOT).contains(needle)) {
                out.add(v);
            }
        }
        return out;
    }

    /** Extract a watch URL from pasted input (watch URL, short youtu.be, shorts, or raw id). */
    public static String normalizeUrl(String input) {
        if (input == null) {
            return "";
        }
        String s = input.trim();
        if (s.isEmpty()) {
            return "";
        }
        if (s.startsWith("http://") || s.startsWith("https://")) {
            return s;
        }
        // Bare 11-char video id
        if (s.matches("[A-Za-z0-9_-]{11}")) {
            return "https://www.youtube.com/watch?v=" + s;
        }
        return s;
    }

    public static String formatViews(long views) {
        if (views >= 1_000_000L) {
            return (views / 1_000_000L) + "M";
        }
        if (views >= 1_000L) {
            return (views / 1_000L) + "K";
        }
        return Long.toString(views);
    }

    private static String nullToEmpty(String s) {
        return s == null ? "" : s;
    }
}
