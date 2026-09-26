package dev.tuffbox.cosmetics;

import dev.tuffbox.cosmetics.core.CosmeticsCore;
import net.minecraft.client.Minecraft;

import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;

/**
 * Client-side GUI overrides. Persisted under {@code .tuffbox/cosmetics-gui.json}
 * and synced to Supabase via {@link CosmeticsCore#upsertProfile} when writeSecret
 * is present in the launch session.
 */
public final class CosmeticsClientConfig {
    public boolean master = true;
    public boolean wingsEnabled = true;
    public boolean hatEnabled = true;
    public boolean trail = true;
    public boolean jumpCircles = true;
    public boolean hitParticles = true;
    public boolean hitBubbles = true;
    public boolean targetEsp = true;
    public boolean killEffect = true;
    /** Empty = keep session / remote value. */
    public String wingsId = "";
    /** Empty = keep session / remote value. */
    public String hatId = "";
    /** When false, still show peers' cosmetics but hide self FX. */
    public boolean showSelf = true;
    /** When false, do not render other players' TuffBox FX. */
    public boolean showPeers = true;

    private static CosmeticsClientConfig INSTANCE;

    private CosmeticsClientConfig() {}

    public static CosmeticsClientConfig get() {
        if (INSTANCE == null) {
            INSTANCE = load();
        }
        return INSTANCE;
    }

    public static void reload() {
        INSTANCE = load();
    }

    /** Drop GUI file and re-seed from cosmetics-session.json. */
    public static void reloadFromSession() {
        try {
            Path path = configPath();
            Files.deleteIfExists(path);
        } catch (Exception ignored) {
        }
        INSTANCE = null;
        CosmeticsClientConfig c = get();
        c.save();
    }

    public void save() {
        try {
            Path path = configPath();
            Files.createDirectories(path.getParent());
            String json = toJson();
            Files.write(path, json.getBytes(StandardCharsets.UTF_8));
        } catch (Exception e) {
            TuffBoxCosmeticsClient.LOG.warn("cosmetics gui save: {}", e.toString());
        }
        syncSessionAndRemote();
    }

    /** Update session file + async upsert so peers see the look. */
    private void syncSessionAndRemote() {
        try {
            Path gameDir = gameDir();
            CosmeticsCore.Session sess = CosmeticsCore.loadSession(gameDir);
            if (sess == null) {
                return;
            }
            sess.wings = wingsEnabled ? (wingsId == null ? "" : wingsId) : "";
            sess.hat = hatEnabled ? (hatId == null ? "" : hatId) : "";
            sess.trail = trail;
            sess.jumpCircles = jumpCircles;
            sess.hitParticles = hitParticles;
            sess.hitBubbles = hitBubbles;
            sess.targetEsp = targetEsp;
            sess.killEffect = killEffect;
            CosmeticsCore.writeSession(gameDir, sess);

            CosmeticsCore.Snapshot snap = CosmeticsCore.snapshotFromSession(sess);
            Thread t = new Thread(() -> {
                boolean ok = CosmeticsCore.upsertProfile(sess, snap);
                if (!ok) {
                    TuffBoxCosmeticsClient.LOG.warn("cosmetics-upsert failed (peers may not see look)");
                }
            }, "tuffbox-cosmetics-upsert");
            t.setDaemon(true);
            t.start();
        } catch (Exception e) {
            TuffBoxCosmeticsClient.LOG.warn("cosmetics sync: {}", e.toString());
        }
    }

    private static CosmeticsClientConfig load() {
        CosmeticsClientConfig c = new CosmeticsClientConfig();
        try {
            Path path = configPath();
            if (!Files.isRegularFile(path)) {
                // Seed from session if present
                CosmeticsCore.Session s = CosmeticsCore.loadSession(gameDir());
                if (s != null) {
                    c.trail = s.trail;
                    c.jumpCircles = s.jumpCircles;
                    c.hitParticles = s.hitParticles;
                    c.hitBubbles = s.hitBubbles;
                    c.targetEsp = s.targetEsp;
                    c.killEffect = s.killEffect;
                    c.wingsId = s.wings == null ? "" : s.wings;
                    c.hatId = s.hat == null ? "" : s.hat;
                    c.wingsEnabled = c.wingsId != null && !c.wingsId.isEmpty();
                    c.hatEnabled = c.hatId != null && !c.hatId.isEmpty();
                }
                return c;
            }
            String text = new String(Files.readAllBytes(path), StandardCharsets.UTF_8);
            c.master = bool(text, "master", true);
            c.wingsEnabled = bool(text, "wingsEnabled", true);
            c.hatEnabled = bool(text, "hatEnabled", true);
            c.trail = bool(text, "trail", true);
            c.jumpCircles = bool(text, "jumpCircles", true);
            c.hitParticles = bool(text, "hitParticles", true);
            c.hitBubbles = bool(text, "hitBubbles", true);
            c.targetEsp = bool(text, "targetEsp", true);
            c.killEffect = bool(text, "killEffect", true);
            c.showSelf = bool(text, "showSelf", true);
            c.showPeers = bool(text, "showPeers", true);
            String w = str(text, "wingsId");
            String h = str(text, "hatId");
            if (w != null) c.wingsId = w;
            if (h != null) c.hatId = h;
        } catch (Exception e) {
            TuffBoxCosmeticsClient.LOG.warn("cosmetics gui load: {}", e.toString());
        }
        return c;
    }

    private String toJson() {
        return "{\n"
                + "  \"master\": " + master + ",\n"
                + "  \"wingsEnabled\": " + wingsEnabled + ",\n"
                + "  \"hatEnabled\": " + hatEnabled + ",\n"
                + "  \"trail\": " + trail + ",\n"
                + "  \"jumpCircles\": " + jumpCircles + ",\n"
                + "  \"hitParticles\": " + hitParticles + ",\n"
                + "  \"hitBubbles\": " + hitBubbles + ",\n"
                + "  \"targetEsp\": " + targetEsp + ",\n"
                + "  \"killEffect\": " + killEffect + ",\n"
                + "  \"showSelf\": " + showSelf + ",\n"
                + "  \"showPeers\": " + showPeers + ",\n"
                + "  \"wingsId\": \"" + esc(wingsId) + "\",\n"
                + "  \"hatId\": \"" + esc(hatId) + "\"\n"
                + "}\n";
    }

    private static String esc(String s) {
        if (s == null) return "";
        return s.replace("\\", "\\\\").replace("\"", "\\\"");
    }

    private static Path gameDir() {
        return Minecraft.getInstance().gameDirectory.toPath();
    }

    private static Path configPath() {
        return gameDir().resolve(".tuffbox").resolve("cosmetics-gui.json");
    }

    private static boolean bool(String json, String key, boolean def) {
        String needle = "\"" + key + "\"";
        int i = json.indexOf(needle);
        if (i < 0) return def;
        int colon = json.indexOf(':', i + needle.length());
        if (colon < 0) return def;
        int p = colon + 1;
        while (p < json.length() && Character.isWhitespace(json.charAt(p))) p++;
        if (json.regionMatches(true, p, "true", 0, 4)) return true;
        if (json.regionMatches(true, p, "false", 0, 5)) return false;
        return def;
    }

    private static String str(String json, String key) {
        String needle = "\"" + key + "\"";
        int i = json.indexOf(needle);
        if (i < 0) return null;
        int colon = json.indexOf(':', i + needle.length());
        if (colon < 0) return null;
        int p = colon + 1;
        while (p < json.length() && Character.isWhitespace(json.charAt(p))) p++;
        if (p >= json.length() || json.charAt(p) != '"') return null;
        p++;
        StringBuilder sb = new StringBuilder();
        while (p < json.length()) {
            char c = json.charAt(p++);
            if (c == '\\' && p < json.length()) {
                sb.append(json.charAt(p++));
                continue;
            }
            if (c == '"') break;
            sb.append(c);
        }
        return sb.toString();
    }

    public static final String[] WINGS = {"", "angel", "demon", "fairy"};
    public static final String[] WINGS_LABELS = {"None", "Angel", "Demon", "Fairy"};
    public static final String[] HATS = {"", "china", "halo", "horns", "crown"};
    public static final String[] HATS_LABELS = {"None", "China hat", "Halo", "Horns", "Crown"};

    public static int indexOf(String[] ids, String id) {
        if (id == null) id = "";
        for (int i = 0; i < ids.length; i++) {
            if (ids[i].equalsIgnoreCase(id)) return i;
        }
        return 0;
    }
}
