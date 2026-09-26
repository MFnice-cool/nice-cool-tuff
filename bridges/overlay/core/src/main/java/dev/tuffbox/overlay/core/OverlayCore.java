package dev.tuffbox.overlay.core;

import java.io.BufferedReader;
import java.io.IOException;
import java.io.InputStreamReader;
import java.io.Reader;
import java.net.HttpURLConnection;
import java.net.URI;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.ArrayList;
import java.util.List;

/**
 * Loader/MC-agnostic overlay protocol (Java 8).
 * Session file written by the launcher at inject time + edge-function HTTP.
 * No Minecraft classes here.
 */
public final class OverlayCore {
    private OverlayCore() {}

    public static final String SESSION_NAME = "overlay-session.json";

    public static class Session {
        public String username;
        public String uuid;
        public String apiBase;
        public String anonKey;
        /** Local write secret (same credential the cosmetics profile uses). */
        public String writeSecret = "";
        /** Pack/instance display name for presence ("Playing X"). */
        public String packName = "";

        public boolean usable() {
            return apiBase != null && !apiBase.isEmpty()
                    && uuid != null && !uuid.isEmpty()
                    && username != null && !username.isEmpty();
        }

        public boolean canWrite() {
            return usable() && writeSecret != null && writeSecret.length() >= 16;
        }
    }

    public static Session loadSession(Path gameDir) {
        if (gameDir == null) {
            return null;
        }
        Path path = gameDir.resolve(".tuffbox").resolve(SESSION_NAME);
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
        s.packName = nullToEmpty(jsonString(json, "packName"));
        return s;
    }

    // ── HTTP ──────────────────────────────────────────────────────────

    /** GET {apiBase}/rest/v1/{tableQuery} — direct PostgREST read (public tables). */
    public static String httpGetRest(Session session, String tableQuery) {
        if (session == null || session.apiBase == null || session.apiBase.isEmpty()) {
            return null;
        }
        HttpURLConnection conn = null;
        try {
            String base = session.apiBase.replaceAll("/$", "");
            String url = base + "/rest/v1/" + tableQuery;
            conn = (HttpURLConnection) URI.create(url).toURL().openConnection();
            conn.setConnectTimeout(5000);
            conn.setReadTimeout(8000);
            conn.setRequestMethod("GET");
            applyAuth(session, conn);
            int code = conn.getResponseCode();
            if (code < 200 || code >= 300) {
                return null;
            }
            return readFully(new InputStreamReader(conn.getInputStream(), StandardCharsets.UTF_8));
        } catch (Exception e) {
            return null;
        } finally {
            if (conn != null) {
                conn.disconnect();
            }
        }
    }

    /** GET {apiBase}/functions/v1/{fn}?{query}. Returns body or null on failure. */
    public static String httpGet(Session session, String fn, String query) {
        if (session == null || session.apiBase == null || session.apiBase.isEmpty()) {
            return null;
        }
        HttpURLConnection conn = null;
        try {
            String base = session.apiBase.replaceAll("/$", "");
            String url = base + "/functions/v1/" + fn + (query == null || query.isEmpty() ? "" : "?" + query);
            conn = (HttpURLConnection) URI.create(url).toURL().openConnection();
            conn.setConnectTimeout(5000);
            conn.setReadTimeout(8000);
            conn.setRequestMethod("GET");
            applyAuth(session, conn);
            int code = conn.getResponseCode();
            if (code == 204) {
                return "";
            }
            if (code < 200 || code >= 300) {
                return null;
            }
            return readFully(new InputStreamReader(conn.getInputStream(), StandardCharsets.UTF_8));
        } catch (Exception e) {
            return null;
        } finally {
            if (conn != null) {
                conn.disconnect();
            }
        }
    }

    /** POST JSON to {apiBase}/functions/v1/{fn}. Returns body or null on failure. */
    public static String httpPost(Session session, String fn, String body) {
        if (session == null || session.apiBase == null || session.apiBase.isEmpty()) {
            return null;
        }
        HttpURLConnection conn = null;
        try {
            String base = session.apiBase.replaceAll("/$", "");
            String url = base + "/functions/v1/" + fn;
            conn = (HttpURLConnection) URI.create(url).toURL().openConnection();
            conn.setConnectTimeout(5000);
            conn.setReadTimeout(10000);
            conn.setRequestMethod("POST");
            conn.setDoOutput(true);
            conn.setRequestProperty("Content-Type", "application/json");
            applyAuth(session, conn);
            byte[] bytes = body.getBytes(StandardCharsets.UTF_8);
            conn.setFixedLengthStreamingMode(bytes.length);
            conn.getOutputStream().write(bytes);
            int code = conn.getResponseCode();
            if (code < 200 || code >= 300) {
                return null;
            }
            return readFully(new InputStreamReader(conn.getInputStream(), StandardCharsets.UTF_8));
        } catch (Exception e) {
            return null;
        } finally {
            if (conn != null) {
                conn.disconnect();
            }
        }
    }

    private static void applyAuth(Session session, HttpURLConnection conn) {
        if (session.anonKey != null && !session.anonKey.isEmpty()) {
            conn.setRequestProperty("apikey", session.anonKey);
            conn.setRequestProperty("Authorization", "Bearer " + session.anonKey);
        }
    }

    // ── JSON helpers (minimal pull parser; keep dependency-free) ─────

    public static String esc(String s) {
        if (s == null) {
            return "";
        }
        StringBuilder sb = new StringBuilder(s.length() + 8);
        for (int i = 0; i < s.length(); i++) {
            char c = s.charAt(i);
            switch (c) {
                case '\\': sb.append("\\\\"); break;
                case '"': sb.append("\\\""); break;
                case '\n': sb.append("\\n"); break;
                case '\r': sb.append("\\r"); break;
                case '\t': sb.append("\\t"); break;
                default: sb.append(c);
            }
        }
        return sb.toString();
    }

    public static String jsonString(String json, String key) {
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
                char e = json.charAt(p++);
                switch (e) {
                    case 'n': sb.append('\n'); break;
                    case 'r': sb.append('\r'); break;
                    case 't': sb.append('\t'); break;
                    default: sb.append(e);
                }
                continue;
            }
            if (c == '"') {
                break;
            }
            sb.append(c);
        }
        return sb.toString();
    }

    public static boolean jsonBool(String json, String key, boolean def) {
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

    public static long jsonLong(String json, String key, long def) {
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
        int start = p;
        while (p < json.length() && (Character.isDigit(json.charAt(p)) || json.charAt(p) == '-')) {
            p++;
        }
        if (p == start) {
            return def;
        }
        try {
            return Long.parseLong(json.substring(start, p));
        } catch (NumberFormatException e) {
            return def;
        }
    }

    /** Extract nested object body for "key": { ... } — braces included. */
    public static String extractObject(String json, String key) {
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
        boolean inStr = false;
        for (int p = brace; p < json.length(); p++) {
            char c = json.charAt(p);
            if (c == '"' && (p == 0 || json.charAt(p - 1) != '\\')) {
                inStr = !inStr;
                continue;
            }
            if (inStr) {
                continue;
            }
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

    /** Extract array body for "key": [ ... ] — brackets included. */
    public static String extractArray(String json, String key) {
        String needle = "\"" + key + "\"";
        int i = json.indexOf(needle);
        if (i < 0) {
            return null;
        }
        int open = json.indexOf('[', i + needle.length());
        if (open < 0) {
            return null;
        }
        int depth = 0;
        boolean inStr = false;
        for (int p = open; p < json.length(); p++) {
            char c = json.charAt(p);
            if (c == '"' && (p == 0 || json.charAt(p - 1) != '\\')) {
                inStr = !inStr;
                continue;
            }
            if (inStr) {
                continue;
            }
            if (c == '[') {
                depth++;
            } else if (c == ']') {
                depth--;
                if (depth == 0) {
                    return json.substring(open, p + 1);
                }
            }
        }
        return null;
    }

    /** Split a JSON array body into top-level object entries ("{...}"). */
    public static List<String> splitObjects(String arrayJson) {
        List<String> out = new ArrayList<String>();
        if (arrayJson == null) {
            return out;
        }
        int depth = 0;
        int start = -1;
        boolean inStr = false;
        for (int p = 0; p < arrayJson.length(); p++) {
            char c = arrayJson.charAt(p);
            if (c == '"' && (p == 0 || arrayJson.charAt(p - 1) != '\\')) {
                inStr = !inStr;
                continue;
            }
            if (inStr) {
                continue;
            }
            if (c == '{') {
                if (depth == 0) {
                    start = p;
                }
                depth++;
            } else if (c == '}') {
                depth--;
                if (depth == 0 && start >= 0) {
                    out.add(arrayJson.substring(start, p + 1));
                    start = -1;
                }
            }
        }
        return out;
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
}
