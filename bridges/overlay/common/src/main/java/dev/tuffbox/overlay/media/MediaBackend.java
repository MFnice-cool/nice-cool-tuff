package dev.tuffbox.overlay.media;

/**
 * Media player abstraction. The WATERMeDIA-backed implementation is loaded
 * reflectively so the overlay still runs (social features intact) when the
 * watermedia jar is not installed.
 */
public interface MediaBackend {

    /** Begin playback of a URL (YouTube watch/shorts or direct media). */
    void play(String url);

    void pausePlayback();

    void resumePlayback();

    void togglePlayback();

    void stopPlayback();

    boolean isPlaying();

    boolean isPaused();

    boolean isBuffering();

    boolean isLoading();

    boolean isEnded();

    boolean isBroken();

    /** GL texture id with the current frame; -1 when nothing to show. */
    int texture();

    /** Video frame width in px (0 before first frame). */
    int videoWidth();

    /** Video frame height in px (0 before first frame). */
    int videoHeight();

    long timeMs();

    long durationMs();

    void seekTo(long ms);

    int volume();

    /** 0..100. */
    void setVolume(int volume);

    /** Player is created but VLC/native side is still warming up. */
    boolean engineLoading();

    /** Native engine (VLC) unavailable on this machine — show a hint. */
    boolean engineMissing();

    /** Free native + GL resources. Player is unusable afterwards. */
    void release();
}
