package dev.tuffbox.overlay;

import dev.tuffbox.overlay.core.OverlayCore;
import dev.tuffbox.overlay.media.MediaController;

import java.util.Map;
import java.util.concurrent.ConcurrentHashMap;

/**
 * Runtime overlay singleton. Lives outside the Screen lifecycle so media keeps
 * playing while the GUI is closed and chat state survives overlay toggles.
 */
public final class OverlayState {
    private static final OverlayState INSTANCE = new OverlayState();

    public static OverlayState get() {
        return INSTANCE;
    }

    private OverlayState() {}

    /** Launch session injected by the launcher; null when running outside TuffBox. */
    public volatile OverlayCore.Session session;

    /** Single media player (YouTube). Survives overlay open/close cycles. */
    public final MediaController media = new MediaController();

    /** Unread chat counters per conversation key (player key). */
    public final Map<String, Integer> unread = new ConcurrentHashMap<String, Integer>();

    /** Sum of all unread counters — badge on the rail. */
    public int totalUnread() {
        int sum = 0;
        for (Integer n : unread.values()) {
            if (n != null) {
                sum += n;
            }
        }
        return sum;
    }

    public void markRead(String conversationKey) {
        if (conversationKey != null) {
            unread.remove(conversationKey);
        }
    }

    public void bumpUnread(String conversationKey) {
        if (conversationKey == null) {
            return;
        }
        Integer n = unread.get(conversationKey);
        unread.put(conversationKey, n == null ? 1 : n + 1);
    }

    /** True when background audio/PiP should render even with no screen open. */
    public boolean hasActiveMedia() {
        return media.isActive();
    }
}
