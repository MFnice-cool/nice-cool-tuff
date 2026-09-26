package dev.tuffbox.overlay.core;

import java.util.ArrayList;
import java.util.List;

/**
 * Social protocol for the in-game overlay: friends, presence, direct chat.
 * Pure Java 8 over OverlayCore's HTTP helpers — no Minecraft classes.
 */
public final class SocialApi {
    private SocialApi() {}

    public static final class Friend {
        public long id;
        public String key = "";
        public String name = "";
        public String since = "";
        public boolean online;
        public String pack = "";
        public String server = "";
    }

    public static final class FriendsSnapshot {
        public boolean ok;
        public final List<Friend> friends = new ArrayList<Friend>();
        public final List<Friend> incoming = new ArrayList<Friend>();
        public final List<Friend> outgoing = new ArrayList<Friend>();
    }

    public static final class ChatMessage {
        public long id;
        public String conversation = "";
        public String fromKey = "";
        public String fromName = "";
        public String toKey = "";
        public String body = "";
        public String at = "";
    }

    public static final class ChatBatch {
        public boolean ok;
        public long cursor;
        public final List<ChatMessage> messages = new ArrayList<ChatMessage>();
    }

    // ── friends ───────────────────────────────────────────────────────

    public static FriendsSnapshot listFriends(OverlayCore.Session s) {
        FriendsSnapshot out = new FriendsSnapshot();
        if (!authReady(s)) {
            return out;
        }
        String body = OverlayCore.httpPost(s, "overlay-friends",
                baseAuth(s, "\"action\":\"list\""));
        if (body == null || !OverlayCore.jsonBool(body, "ok", false)) {
            return out;
        }
        out.ok = true;
        fillFriends(OverlayCore.extractArray(body, "friends"), out.friends);
        fillFriends(OverlayCore.extractArray(body, "incoming"), out.incoming);
        fillFriends(OverlayCore.extractArray(body, "outgoing"), out.outgoing);
        return out;
    }

    /** Add by username. Returns "accepted" | "sent" | "already:<status>" or null on failure. */
    public static String addFriend(OverlayCore.Session s, String friendUsername) {
        if (!authReady(s) || friendUsername == null || friendUsername.trim().isEmpty()) {
            return null;
        }
        String body = OverlayCore.httpPost(s, "overlay-friends",
                baseAuth(s, "\"action\":\"add\",\"friendUsername\":\""
                        + OverlayCore.esc(friendUsername.trim()) + "\""));
        if (body == null || !OverlayCore.jsonBool(body, "ok", false)) {
            return null;
        }
        if (OverlayCore.jsonBool(body, "accepted", false)) {
            return "accepted";
        }
        if (OverlayCore.jsonBool(body, "already", false)) {
            String st = OverlayCore.jsonString(body, "status");
            return "already:" + (st == null ? "pending" : st);
        }
        return "sent";
    }

    public static boolean acceptFriend(OverlayCore.Session s, long friendshipId) {
        if (!authReady(s)) {
            return false;
        }
        String body = OverlayCore.httpPost(s, "overlay-friends",
                baseAuth(s, "\"action\":\"accept\",\"friendshipId\":" + friendshipId));
        return body != null && OverlayCore.jsonBool(body, "ok", false);
    }

    public static boolean removeFriend(OverlayCore.Session s, long friendshipId) {
        if (!authReady(s)) {
            return false;
        }
        String body = OverlayCore.httpPost(s, "overlay-friends",
                baseAuth(s, "\"action\":\"remove\",\"friendshipId\":" + friendshipId));
        return body != null && OverlayCore.jsonBool(body, "ok", false);
    }

    // ── presence ──────────────────────────────────────────────────────

    /** Heartbeat. Returns live presence rows of accepted friends (null on failure). */
    public static List<Friend> heartbeat(OverlayCore.Session s, String packName,
                                         String server, boolean offline) {
        if (!authReady(s)) {
            return null;
        }
        String extra = "\"packName\":\"" + OverlayCore.esc(packName) + "\","
                + "\"server\":\"" + OverlayCore.esc(server) + "\","
                + "\"offline\":" + offline;
        String body = OverlayCore.httpPost(s, "overlay-presence", baseAuth(s, extra));
        if (body == null || !OverlayCore.jsonBool(body, "ok", false)) {
            return null;
        }
        List<Friend> out = new ArrayList<Friend>();
        for (String obj : OverlayCore.splitObjects(OverlayCore.extractArray(body, "friends"))) {
            Friend f = new Friend();
            f.key = nullToEmpty(OverlayCore.jsonString(obj, "key"));
            f.name = nullToEmpty(OverlayCore.jsonString(obj, "name"));
            f.pack = nullToEmpty(OverlayCore.jsonString(obj, "pack"));
            f.server = nullToEmpty(OverlayCore.jsonString(obj, "server"));
            f.online = true;
            out.add(f);
        }
        return out;
    }

    // ── chat ──────────────────────────────────────────────────────────

    /** Send a DM. Returns message id > 0 on success, -1 otherwise. */
    public static long sendChat(OverlayCore.Session s, String toKey, String text) {
        if (!authReady(s) || toKey == null || toKey.isEmpty()
                || text == null || text.trim().isEmpty()) {
            return -1;
        }
        String trimmed = text.trim();
        if (trimmed.length() > 500) {
            trimmed = trimmed.substring(0, 500);
        }
        String body = OverlayCore.httpPost(s, "overlay-chat-send",
                baseAuth(s, "\"toKey\":\"" + OverlayCore.esc(toKey) + "\","
                        + "\"body\":\"" + OverlayCore.esc(trimmed) + "\""));
        if (body == null || !OverlayCore.jsonBool(body, "ok", false)) {
            return -1;
        }
        return OverlayCore.jsonLong(body, "id", -1);
    }

    /** Incremental poll: messages with id > sinceId that involve me. */
    public static ChatBatch pollChat(OverlayCore.Session s, long sinceId) {
        ChatBatch out = new ChatBatch();
        out.cursor = sinceId;
        if (!authReady(s)) {
            return out;
        }
        String body = OverlayCore.httpPost(s, "overlay-chat-poll",
                baseAuth(s, "\"sinceId\":" + sinceId));
        if (body == null || !OverlayCore.jsonBool(body, "ok", false)) {
            return out;
        }
        out.ok = true;
        out.cursor = OverlayCore.jsonLong(body, "cursor", sinceId);
        for (String obj : OverlayCore.splitObjects(OverlayCore.extractArray(body, "messages"))) {
            ChatMessage m = new ChatMessage();
            m.id = OverlayCore.jsonLong(obj, "id", 0);
            m.conversation = nullToEmpty(OverlayCore.jsonString(obj, "conversation"));
            m.fromKey = nullToEmpty(OverlayCore.jsonString(obj, "fromKey"));
            m.fromName = nullToEmpty(OverlayCore.jsonString(obj, "fromName"));
            m.toKey = nullToEmpty(OverlayCore.jsonString(obj, "toKey"));
            m.body = nullToEmpty(OverlayCore.jsonString(obj, "body"));
            m.at = nullToEmpty(OverlayCore.jsonString(obj, "at"));
            if (m.id > 0) {
                out.messages.add(m);
            }
        }
        return out;
    }

    // ── helpers ───────────────────────────────────────────────────────

    private static void fillFriends(String arrayJson, List<Friend> out) {
        for (String obj : OverlayCore.splitObjects(arrayJson)) {
            Friend f = new Friend();
            f.id = OverlayCore.jsonLong(obj, "id", 0);
            f.key = nullToEmpty(OverlayCore.jsonString(obj, "key"));
            f.name = nullToEmpty(OverlayCore.jsonString(obj, "name"));
            f.since = nullToEmpty(OverlayCore.jsonString(obj, "since"));
            out.add(f);
        }
    }

    private static boolean authReady(OverlayCore.Session s) {
        return s != null && s.canWrite();
    }

    private static String baseAuth(OverlayCore.Session s, String extra) {
        return "{\"playerKey\":\"" + OverlayCore.esc(s.uuid) + "\","
                + "\"username\":\"" + OverlayCore.esc(s.username) + "\","
                + "\"writeSecret\":\"" + OverlayCore.esc(s.writeSecret) + "\","
                + extra + "}";
    }

    private static String nullToEmpty(String s) {
        return s == null ? "" : s;
    }
}
