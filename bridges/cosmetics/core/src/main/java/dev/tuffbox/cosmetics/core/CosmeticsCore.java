package dev.tuffbox.cosmetics.core;

import java.io.BufferedReader;
import java.io.IOException;
import java.io.InputStreamReader;
import java.io.Reader;
import java.net.HttpURLConnection;
import java.net.URI;
import java.net.URLEncoder;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.Locale;
import java.util.Map;
import java.util.concurrent.ConcurrentHashMap;

/**
 * Loader/MC-agnostic cosmetics protocol (Java 8).
 * Session file + remote profile fetch — no Minecraft classes.
 */
public final class CosmeticsCore {
    private CosmeticsCore() {}

    public static class Session {
        public String username;
        public String uuid;
        public String apiBase;
        public String anonKey;
        /** Local write secret for cosmetics-upsert (from launcher profile). */
        public String writeSecret = "";
        public String wings = "";
        public String hat = "";
        public boolean trail;
        public boolean jumpCircles;
        public boolean hitParticles = true;
        public boolean hitBubbles = true;
        public boolean targetEsp = true;
        public boolean killEffect = true;
    }

    public static class Snapshot {
        public String wings = "";
        public String hat = "";
        public boolean trail;
        public boolean jumpCircles;
        public boolean hitParticles = true;
        public boolean hitBubbles = true;
        public boolean targetEsp = true;
        public boolean killEffect = true;
    }

    public static final Map<String, Snapshot> BY_NAME = new ConcurrentHashMap<String, Snapshot>();

    public static Session loadSession(Path gameDir) {
        if (gameDir == null) {
            return null;
        }
        Path path = gameDir.resolve(".tuffbox").resolve("cosmetics-session.json");
        if (!Files.isRegularFile(path)) {
            return null;
        }
        try {
            String text = new String(Files.readAllBytes(path), StandardCharsets.UTF_8);
            return parseSession(text);
        } catch (IOException e) {
            return null;
        }
    }

    /** Minimal JSON pull for known flat keys (no Gson dependency in core). */
    public static Session parseSession(String json) {
        if (json == null || json.trim().isEmpty()) {
            return null;
        }
        Session s = new Session();
        s.username = jsonString(json, "username");
        s.uuid = jsonString(json, "uuid");
        s.apiBase = jsonString(json, "apiBase");
        s.anonKey = jsonString(json, "anonKey");
        s.writeSecret = nullToEmpty(jsonString(json, "writeSecret"));
        s.wings = nullToEmpty(jsonString(json, "wings"));
        s.hat = nullToEmpty(jsonString(json, "hat"));
        s.trail = jsonBool(json, "trail", false);
        s.jumpCircles = jsonBool(json, "jumpCircles", false);
        s.hitParticles = jsonBool(json, "hitParticles", true);
        s.hitBubbles = jsonBool(json, "hitBubbles", true);
        s.targetEsp = jsonBool(json, "targetEsp", true);
        s.killEffect = jsonBool(json, "killEffect", true);
        return s;
    }

    public static Snapshot snapshotFromSession(Session sess) {
        if (sess == null) {
            return null;
        }
        Snapshot local = new Snapshot();
        local.wings = nullToEmpty(sess.wings);
        local.hat = nullToEmpty(sess.hat);
        local.trail = sess.trail;
        local.jumpCircles = sess.jumpCircles;
        local.hitParticles = sess.hitParticles;
        local.hitBubbles = sess.hitBubbles;
        local.targetEsp = sess.targetEsp;
        local.killEffect = sess.killEffect;
        return local;
    }

    public static Snapshot get(String username) {
        if (username == null) {
            return null;
        }
        return BY_NAME.get(username.toLowerCase(Locale.ROOT));
    }

    public static void put(String username, Snapshot snap) {
        if (username == null || snap == null) {
            return;
        }
        BY_NAME.put(username.toLowerCase(Locale.ROOT), snap);
    }

    public static Snapshot fetchProfile(Session session, String username) {
        Snapshot snap = new Snapshot();
        if (session == null || session.apiBase == null || session.apiBase.isEmpty()) {
            return snap;
        }
        HttpURLConnection conn = null;
        try {
            String base = session.apiBase.replaceAll("/$", "");
            String url = base
                    + "/functions/v1/cosmetics-get?username="
                    + URLEncoder.encode(username, "UTF-8");
            conn = (HttpURLConnection) URI.create(url).toURL().openConnection();
            conn.setConnectTimeout(5000);
            conn.setReadTimeout(8000);
            conn.setRequestMethod("GET");
            if (session.anonKey != null && !session.anonKey.isEmpty()) {
                conn.setRequestProperty("apikey", session.anonKey);
                conn.setRequestProperty("Authorization", "Bearer " + session.anonKey);
            }
            int code = conn.getResponseCode();
            if (code != 200) {
                return snap;
            }
            String body = readFully(new InputStreamReader(conn.getInputStream(), StandardCharsets.UTF_8));
            applyCosmeticsObject(body, snap);
        } catch (Exception ignored) {
            // keep empty snap
        } finally {
            if (conn != null) {
                conn.disconnect();
            }
        }
        return snap;
    }

    /**
     * Push local cosmetics to Supabase so peers can fetch via cosmetics-get.
     * Requires writeSecret from the launch session (launcher profile).
     */
    public static boolean upsertProfile(Session session, Snapshot snap) {
        if (session == null || snap == null) {
            return false;
        }
        if (session.apiBase == null || session.apiBase.isEmpty()) {
            return false;
        }
        if (session.writeSecret == null || session.writeSecret.length() < 16) {
            return false;
        }
        if (session.uuid == null || session.uuid.isEmpty()) {
            return false;
        }
        if (session.username == null || session.username.isEmpty()) {
            return false;
        }
        HttpURLConnection conn = null;
        try {
            String base = session.apiBase.replaceAll("/$", "");
            String url = base + "/functions/v1/cosmetics-upsert";
            String wings = snap.wings == null ? "" : snap.wings;
            String hat = snap.hat == null ? "" : snap.hat;
            String body = "{"
                    + "\"playerKey\":\"" + esc(session.uuid) + "\","
                    + "\"username\":\"" + esc(session.username) + "\","
                    + "\"writeSecret\":\"" + esc(session.writeSecret) + "\","
                    + "\"skinModel\":\"classic\","
                    + "\"sharePublic\":true,"
                    + "\"capeMeta\":{},"
                    + "\"cosmetics\":{"
                    + "\"wings\":" + strOrNull(wings) + ","
                    + "\"hat\":" + strOrNull(hat) + ","
                    + "\"trail\":" + snap.trail + ","
                    + "\"jumpCircles\":" + snap.jumpCircles + ","
                    + "\"hitParticles\":" + snap.hitParticles + ","
                    + "\"hitBubbles\":" + snap.hitBubbles + ","
                    + "\"targetEsp\":" + snap.targetEsp + ","
                    + "\"killEffect\":" + snap.killEffect
                    + "}"
                    + "}";
            conn = (HttpURLConnection) URI.create(url).toURL().openConnection();
            conn.setConnectTimeout(5000);
            conn.setReadTimeout(12000);
            conn.setRequestMethod("POST");
            conn.setDoOutput(true);
            conn.setRequestProperty("Content-Type", "application/json");
            if (session.anonKey != null && !session.anonKey.isEmpty()) {
                conn.setRequestProperty("apikey", session.anonKey);
                conn.setRequestProperty("Authorization", "Bearer " + session.anonKey);
            }
            byte[] bytes = body.getBytes(StandardCharsets.UTF_8);
            conn.setFixedLengthStreamingMode(bytes.length);
            conn.getOutputStream().write(bytes);
            int code = conn.getResponseCode();
            return code >= 200 && code < 300;
        } catch (Exception e) {
            return false;
        } finally {
            if (conn != null) {
                conn.disconnect();
            }
        }
    }

    /** Rewrite cosmetics-session.json so next launch inject picks GUI choices. */
    public static void writeSession(Path gameDir, Session session) {
        if (gameDir == null || session == null) {
            return;
        }
        try {
            Path dir = gameDir.resolve(".tuffbox");
            Files.createDirectories(dir);
            Path path = dir.resolve("cosmetics-session.json");
            String json = "{\n"
                    + "  \"username\": \"" + esc(session.username) + "\",\n"
                    + "  \"uuid\": \"" + esc(session.uuid) + "\",\n"
                    + "  \"apiBase\": \"" + esc(session.apiBase) + "\",\n"
                    + "  \"anonKey\": \"" + esc(session.anonKey) + "\",\n"
                    + "  \"writeSecret\": \"" + esc(session.writeSecret) + "\",\n"
                    + "  \"wings\": \"" + esc(session.wings) + "\",\n"
                    + "  \"hat\": \"" + esc(session.hat) + "\",\n"
                    + "  \"trail\": " + session.trail + ",\n"
                    + "  \"jumpCircles\": " + session.jumpCircles + ",\n"
                    + "  \"hitParticles\": " + session.hitParticles + ",\n"
                    + "  \"hitBubbles\": " + session.hitBubbles + ",\n"
                    + "  \"targetEsp\": " + session.targetEsp + ",\n"
                    + "  \"killEffect\": " + session.killEffect + "\n"
                    + "}\n";
            Files.write(path, json.getBytes(StandardCharsets.UTF_8));
        } catch (Exception ignored) {
        }
    }

    private static String esc(String s) {
        if (s == null) {
            return "";
        }
        return s.replace("\\", "\\\\").replace("\"", "\\\"");
    }

    private static String strOrNull(String s) {
        if (s == null || s.isEmpty()) {
            return "null";
        }
        return "\"" + esc(s) + "\"";
    }

    static void applyCosmeticsObject(String body, Snapshot snap) {
        // Prefer nested "cosmetics": { ... }
        String block = extractObject(body, "cosmetics");
        String src = block != null ? block : body;
        String wings = jsonString(src, "wings");
        if (wings != null) {
            snap.wings = wings;
        }
        String hat = jsonString(src, "hat");
        if (hat != null) {
            snap.hat = hat;
        }
        snap.trail = jsonBool(src, "trail", snap.trail);
        snap.jumpCircles = jsonBool(src, "jumpCircles", snap.jumpCircles);
        snap.hitParticles = jsonBool(src, "hitParticles", snap.hitParticles);
        snap.hitBubbles = jsonBool(src, "hitBubbles", snap.hitBubbles);
        snap.targetEsp = jsonBool(src, "targetEsp", snap.targetEsp);
        snap.killEffect = jsonBool(src, "killEffect", snap.killEffect);
    }

    /** HSV → packed ARGB (a 0–255). */
    public static int hsva(float hue, float sat, float val, int alpha) {
        float h = ((hue % 1f) + 1f) % 1f;
        int i = (int) (h * 6f);
        float f = h * 6f - i;
        float p = val * (1f - sat);
        float q = val * (1f - f * sat);
        float t = val * (1f - (1f - f) * sat);
        float r;
        float g;
        float b;
        switch (i % 6) {
            case 0:
                r = val;
                g = t;
                b = p;
                break;
            case 1:
                r = q;
                g = val;
                b = p;
                break;
            case 2:
                r = p;
                g = val;
                b = t;
                break;
            case 3:
                r = p;
                g = q;
                b = val;
                break;
            case 4:
                r = t;
                g = p;
                b = val;
                break;
            default:
                r = val;
                g = p;
                b = q;
                break;
        }
        return (alpha << 24)
                | ((int) (r * 255) << 16)
                | ((int) (g * 255) << 8)
                | (int) (b * 255);
    }

    private static String nullToEmpty(String s) {
        return s == null ? "" : s;
    }

    private static String readFully(Reader r) throws IOException {
        BufferedReader br = new BufferedReader(r);
        StringBuilder sb = new StringBuilder();
        String line;
        while ((line = br.readLine()) != null) {
            sb.append(line).append('\n');
        }
        return sb.toString();
    }

    private static String jsonString(String json, String key) {
        String needle = "\"" + key + "\"";
        int i = json.indexOf(needle);
        if (i < 0) {
            return null;
        }
        int colon = json.indexOf(':', i + needle.length());
        if (colon < 0) {
            return null;
        }
        int p = colon + 1;
        while (p < json.length() && Character.isWhitespace(json.charAt(p))) {
            p++;
        }
        if (p >= json.length()) {
            return null;
        }
        if (json.charAt(p) == 'n') {
            return null; // null
        }
        if (json.charAt(p) != '"') {
            return null;
        }
        p++;
        StringBuilder sb = new StringBuilder();
        while (p < json.length()) {
            char c = json.charAt(p++);
            if (c == '\\' && p < json.length()) {
                sb.append(json.charAt(p++));
                continue;
            }
            if (c == '"') {
                break;
            }
            sb.append(c);
        }
        return sb.toString();
    }

    private static boolean jsonBool(String json, String key, boolean def) {
        String needle = "\"" + key + "\"";
        int i = json.indexOf(needle);
        if (i < 0) {
            return def;
        }
        int colon = json.indexOf(':', i + needle.length());
        if (colon < 0) {
            return def;
        }
        int p = colon + 1;
        while (p < json.length() && Character.isWhitespace(json.charAt(p))) {
            p++;
        }
        if (json.regionMatches(true, p, "true", 0, 4)) {
            return true;
        }
        if (json.regionMatches(true, p, "false", 0, 5)) {
            return false;
        }
        return def;
    }

    private static String extractObject(String json, String key) {
        String needle = "\"" + key + "\"";
        int i = json.indexOf(needle);
        if (i < 0) {
            return null;
        }
        int brace = json.indexOf('{', i + needle.length());
        if (brace < 0) {
            return null;
        }
        int depth = 0;
        for (int p = brace; p < json.length(); p++) {
            char c = json.charAt(p);
            if (c == '{') {
                depth++;
            } else if (c == '}') {
                depth--;
                if (depth == 0) {
                    return json.substring(brace, p + 1);
                }
            }
        }
        return null;
    }
}
