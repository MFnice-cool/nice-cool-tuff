package dev.tuffbox.overlay.media;

import dev.tuffbox.overlay.OverlayConfig;
import org.slf4j.Logger;
import org.slf4j.LoggerFactory;

import java.util.ArrayDeque;
import java.util.Deque;

/**
 * Owns the single overlay media player: current track, queue, volume.
 * Lives in OverlayState (outside the Screen lifecycle) — this is what lets
 * audio keep playing while the GUI is closed, with the PiP HUD on top.
 */
public final class MediaController {
    private static final Logger LOG = LoggerFactory.getLogger("tuffbox_overlay");

    private static final class QueueEntry {
        final String url;
        final String title;

        QueueEntry(String url, String title) {
            this.url = url;
            this.title = title;
        }
    }

    private MediaBackend backend;
    private boolean backendMissing;
    private final Deque<QueueEntry> queue = new ArrayDeque<QueueEntry>();

    private String currentUrl = "";
    private String currentTitle = "";
    /** Set on play(); cleared when the frame pipeline produced its first texture. */
    private boolean awaitingFirstFrame;

    public synchronized boolean watermediaMissing() {
        ensureBackend();
        return backendMissing;
    }

    public synchronized boolean engineLoading() {
        MediaBackend b = ensureBackend();
        return b != null && b.engineLoading();
    }

    private MediaBackend ensureBackend() {
        if (backend == null && !backendMissing) {
            backend = MediaBackends.create();
            if (backend == null) {
                backendMissing = true;
                LOG.warn("WATERMeDIA not installed — YouTube playback disabled for this session");
            } else {
                backend.setVolume(OverlayConfig.get().volume);
            }
        }
        return backend;
    }

    public synchronized void play(String url, String title) {
        if (url == null || url.isEmpty()) {
            return;
        }
        MediaBackend b = ensureBackend();
        if (b == null) {
            return;
        }
        currentUrl = url;
        currentTitle = title == null ? "" : title;
        awaitingFirstFrame = true;
        b.setVolume(OverlayConfig.get().volume);
        b.play(url);
    }

    public synchronized void enqueue(String url, String title) {
        if (url == null || url.isEmpty()) {
            return;
        }
        queue.addLast(new QueueEntry(url, title == null ? "" : title));
    }

    public synchronized int queueSize() {
        return queue.size();
    }

    public synchronized String queueTitleAt(int index) {
        int i = 0;
        for (QueueEntry e : queue) {
            if (i++ == index) {
                return e.title;
            }
        }
        return "";
    }

    public synchronized void clearQueue() {
        queue.clear();
    }

    public synchronized void toggle() {
        MediaBackend b = ensureBackend();
        if (b != null) {
            b.togglePlayback();
        }
    }

    public synchronized void stop() {
        if (backend != null) {
            backend.stopPlayback();
        }
        currentUrl = "";
        currentTitle = "";
        queue.clear();
    }

    public synchronized void next() {
        QueueEntry e = queue.pollFirst();
        if (e != null) {
            play(e.url, e.title);
        } else {
            stop();
        }
    }

    public synchronized void volumeUp() {
        setVolume(OverlayConfig.get().volume + 5);
    }

    public synchronized void volumeDown() {
        setVolume(OverlayConfig.get().volume - 5);
    }

    public synchronized void setVolume(int v) {
        v = Math.max(0, Math.min(100, v));
        OverlayConfig.get().volume = v;
        OverlayConfig.get().save();
        if (backend != null) {
            backend.setVolume(v);
        }
    }

    public synchronized String currentTitle() {
        return currentTitle;
    }

    public synchronized String currentUrl() {
        return currentUrl;
    }

    /** Something is loaded and audible (playing or paused mid-track). */
    public synchronized boolean isActive() {
        return backend != null && !currentUrl.isEmpty()
                && (backend.isPlaying() || backend.isPaused() || backend.isBuffering() || backend.isLoading());
    }

    public synchronized boolean isPlaying() {
        return backend != null && backend.isPlaying();
    }

    public synchronized boolean isPaused() {
        return backend != null && backend.isPaused();
    }

    public synchronized boolean isBuffering() {
        return backend != null && (backend.isBuffering() || backend.isLoading() || awaitingFirstFrame);
    }

    public synchronized long timeMs() {
        return backend != null ? backend.timeMs() : 0L;
    }

    public synchronized long durationMs() {
        return backend != null ? backend.durationMs() : 0L;
    }

    public synchronized void seekTo(long ms) {
        if (backend != null) {
            backend.seekTo(ms);
        }
    }

    /** Texture for the current frame, or -1. */
    public synchronized int texture() {
        return backend != null ? backend.texture() : -1;
    }

    public synchronized int videoWidth() {
        return backend != null ? backend.videoWidth() : 0;
    }

    public synchronized int videoHeight() {
        return backend != null ? backend.videoHeight() : 0;
    }

    /** Per-client-tick housekeeping: auto-advance the queue when a track ends. */
    public synchronized void tick() {
        MediaBackend b = backend;
        if (b == null) {
            return;
        }
        if (awaitingFirstFrame && b.videoWidth() > 0) {
            awaitingFirstFrame = false;
        }
        if (!currentUrl.isEmpty() && b.isEnded()) {
            next();
        }
    }

    public synchronized void release() {
        if (backend != null) {
            backend.release();
            backend = null;
        }
        queue.clear();
        currentUrl = "";
        currentTitle = "";
    }
}
