package dev.tuffbox.overlay.media.watermedia;

import dev.tuffbox.overlay.media.MediaBackend;
import org.watermedia.api.player.PlayerAPI;
import org.watermedia.api.player.videolan.VideoPlayer;

import java.net.URI;
import java.util.concurrent.Executor;

/**
 * WATERMeDIA 2.1.x backend. YouTube URLs are resolved by WATERMeDIA's
 * built-in network patchers (JYD); VLC natives are extracted by WATERMeDIA
 * itself on Windows x64.
 */
public final class WaterMediaBackend implements MediaBackend {
    private final Executor renderExecutor;
    private VideoPlayer player;
    private int volume = 50;

    public WaterMediaBackend(Executor renderExecutor) {
        this.renderExecutor = renderExecutor;
    }

    private boolean ensurePlayer() {
        if (player != null) {
            return true;
        }
        if (!PlayerAPI.isReady()) {
            return false;
        }
        try {
            player = new VideoPlayer(renderExecutor);
            player.setVolume(volume);
            return true;
        } catch (Throwable t) {
            player = null;
            return false;
        }
    }

    @Override
    public void play(String url) {
        if (!ensurePlayer()) {
            return;
        }
        try {
            player.start(URI.create(url), new String[]{":network-caching=1500"});
        } catch (Exception ignored) {
        }
    }

    @Override
    public void pausePlayback() {
        if (player != null) player.pause();
    }

    @Override
    public void resumePlayback() {
        if (player != null) player.play();
    }

    @Override
    public void togglePlayback() {
        if (player != null) player.togglePlayback();
    }

    @Override
    public void stopPlayback() {
        if (player != null) player.stop();
    }

    @Override
    public boolean isPlaying() {
        return player != null && player.isPlaying();
    }

    @Override
    public boolean isPaused() {
        return player != null && player.isPaused();
    }

    @Override
    public boolean isBuffering() {
        return player != null && player.isBuffering();
    }

    @Override
    public boolean isLoading() {
        return player != null && player.isLoading();
    }

    @Override
    public boolean isEnded() {
        return player != null && player.isEnded();
    }

    @Override
    public boolean isBroken() {
        return player != null && player.isBroken();
    }

    @Override
    public int texture() {
        return player != null ? player.texture() : -1;
    }

    @Override
    public int videoWidth() {
        return player != null ? player.width() : 0;
    }

    @Override
    public int videoHeight() {
        return player != null ? player.height() : 0;
    }

    @Override
    public long timeMs() {
        return player != null ? player.getTime() : 0L;
    }

    @Override
    public long durationMs() {
        return player != null ? player.getDuration() : 0L;
    }

    @Override
    public void seekTo(long ms) {
        if (player != null && player.isSeekAble()) {
            player.seekTo(ms);
        }
    }

    @Override
    public int volume() {
        return volume;
    }

    @Override
    public void setVolume(int v) {
        volume = Math.max(0, Math.min(100, v));
        if (player != null) {
            player.setVolume(volume);
        }
    }

    @Override
    public boolean engineLoading() {
        return player == null && !PlayerAPI.isReady();
    }

    @Override
    public boolean engineMissing() {
        return false; // isReady() stays false; engineLoading covers the wait/fail state
    }

    @Override
    public void release() {
        if (player != null) {
            VideoPlayer ref = player;
            player = null;
            ref.release();
        }
    }
}
