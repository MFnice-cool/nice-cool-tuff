package dev.tuffbox.overlay.social;

import dev.tuffbox.overlay.OverlayConfig;
import dev.tuffbox.overlay.OverlayState;
import dev.tuffbox.overlay.core.OverlayCore;
import dev.tuffbox.overlay.core.SocialApi;
import net.minecraft.client.Minecraft;
import org.slf4j.Logger;
import org.slf4j.LoggerFactory;

import java.util.ArrayList;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;
import java.util.concurrent.ConcurrentHashMap;

/**
 * Background social loop: presence heartbeat (~30s), friends refresh (~20s),
 * chat poll (~4s). Network runs on virtual threads; results land in volatile
 * snapshots consumed by the panels on the render thread.
 */
public final class SocialService {
    private static final Logger LOG = LoggerFactory.getLogger("tuffbox_overlay");

    private static final long HEARTBEAT_MS = 30_000;
    private static final long FRIENDS_MS = 20_000;
    private static final long CHAT_MS = 4_000;

    private volatile SocialApi.FriendsSnapshot friends = new SocialApi.FriendsSnapshot();
    private final Map<String, List<SocialApi.ChatMessage>> conversations =
            new ConcurrentHashMap<String, List<SocialApi.ChatMessage>>();
    private volatile long chatCursor;
    /** Peer key currently open in the chat panel — its messages skip unread badges. */
    private volatile String openConversationWith = "";

    private long lastHeartbeat;
    private long lastFriends;
    private long lastChat;
    private boolean heartbeatRunning;
    private boolean friendsRunning;
    private boolean chatRunning;

    private static final SocialService INSTANCE = new SocialService();

    public static SocialService get() {
        return INSTANCE;
    }

    private SocialService() {}

    public boolean available() {
        OverlayCore.Session s = OverlayState.get().session;
        return s != null && s.canWrite();
    }

    public SocialApi.FriendsSnapshot friends() {
        return friends;
    }

    /** Messages with a peer, oldest first (defensive copy). */
    public List<SocialApi.ChatMessage> conversationWith(String peerKey) {
        List<SocialApi.ChatMessage> list = conversations.get(peerKey);
        if (list == null) {
            return new ArrayList<SocialApi.ChatMessage>();
        }
        synchronized (list) {
            return new ArrayList<SocialApi.ChatMessage>(list);
        }
    }

    public void setOpenConversation(String peerKey) {
        openConversationWith = peerKey == null ? "" : peerKey;
        if (!openConversationWith.isEmpty()) {
            OverlayState.get().markRead(openConversationWith);
        }
    }

    public void tick() {
        if (!available()) {
            return;
        }
        long now = System.currentTimeMillis();
        if (now - lastHeartbeat >= HEARTBEAT_MS && !heartbeatRunning) {
            lastHeartbeat = now;
            heartbeatRunning = true;
            Thread.startVirtualThread(this::runHeartbeat);
        }
        if (now - lastFriends >= FRIENDS_MS && !friendsRunning) {
            lastFriends = now;
            friendsRunning = true;
            Thread.startVirtualThread(this::runFriendsRefresh);
        }
        if (now - lastChat >= CHAT_MS && !chatRunning) {
            lastChat = now;
            chatRunning = true;
            Thread.startVirtualThread(this::runChatPoll);
        }
    }

    /** Force-refresh friends (after add/accept/remove from the panel). */
    public void refreshFriendsAsync() {
        if (!available() || friendsRunning) {
            return;
        }
        friendsRunning = true;
        Thread.startVirtualThread(this::runFriendsRefresh);
    }

    public void sendAsync(String toKey, String text, Runnable after) {
        Thread.startVirtualThread(() -> {
            long id = SocialApi.sendChat(OverlayState.get().session, toKey, text);
            if (id > 0) {
                SocialApi.ChatMessage mine = new SocialApi.ChatMessage();
                mine.id = id;
                mine.conversation = toKey;
                mine.fromKey = OverlayState.get().session.uuid;
                mine.fromName = OverlayState.get().session.username;
                mine.toKey = toKey;
                mine.body = text.length() > 500 ? text.substring(0, 500) : text;
                appendMessage(toKey, mine);
            }
            if (after != null) {
                after.run();
            }
        });
    }

    // ── internals ─────────────────────────────────────────────────────

    private void runHeartbeat() {
        try {
            OverlayCore.Session s = OverlayState.get().session;
            boolean optIn = OverlayConfig.get().presenceOptIn;
            List<SocialApi.Friend> live = SocialApi.heartbeat(
                    s, s.packName, currentServer(), !optIn);
            if (live != null) {
                mergePresence(live);
            }
        } catch (Throwable t) {
            LOG.debug("presence heartbeat failed: {}", t.toString());
        } finally {
            heartbeatRunning = false;
        }
    }

    private void runFriendsRefresh() {
        try {
            SocialApi.FriendsSnapshot snap = SocialApi.listFriends(OverlayState.get().session);
            if (snap.ok) {
                // Keep presence flags we already know until the next heartbeat merges.
                friends = snap;
            }
        } catch (Throwable t) {
            LOG.debug("friends refresh failed: {}", t.toString());
        } finally {
            friendsRunning = false;
        }
    }

    private void runChatPoll() {
        try {
            SocialApi.ChatBatch batch =
                    SocialApi.pollChat(OverlayState.get().session, chatCursor);
            if (!batch.ok) {
                return;
            }
            chatCursor = Math.max(chatCursor, batch.cursor);
            String myKey = OverlayState.get().session.uuid;
            for (SocialApi.ChatMessage m : batch.messages) {
                String peer = m.fromKey.equals(myKey) ? m.toKey : m.fromKey;
                appendMessage(peer, m);
                if (!m.fromKey.equals(myKey) && !peer.equals(openConversationWith)) {
                    OverlayState.get().bumpUnread(peer);
                }
            }
        } catch (Throwable t) {
            LOG.debug("chat poll failed: {}", t.toString());
        } finally {
            chatRunning = false;
        }
    }

    private void appendMessage(String peerKey, SocialApi.ChatMessage m) {
        List<SocialApi.ChatMessage> list = conversations.computeIfAbsent(
                peerKey, k -> new ArrayList<SocialApi.ChatMessage>());
        synchronized (list) {
            for (SocialApi.ChatMessage existing : list) {
                if (existing.id == m.id) {
                    return; // dedupe (own sends vs poll echo)
                }
            }
            list.add(m);
            // Cap history per conversation to keep memory bounded.
            while (list.size() > 400) {
                list.remove(0);
            }
        }
    }

    private void mergePresence(List<SocialApi.Friend> live) {
        Map<String, SocialApi.Friend> byKey = new LinkedHashMap<String, SocialApi.Friend>();
        for (SocialApi.Friend f : live) {
            byKey.put(f.key, f);
        }
        SocialApi.FriendsSnapshot snap = friends;
        for (SocialApi.Friend f : snap.friends) {
            SocialApi.Friend p = byKey.get(f.key);
            f.online = p != null;
            if (p != null) {
                f.pack = p.pack;
                f.server = p.server;
            }
        }
        friends = snap;
    }

    private static String currentServer() {
        Minecraft mc = Minecraft.getInstance();
        if (mc.getCurrentServer() != null) {
            return mc.getCurrentServer().ip;
        }
        if (mc.hasSingleplayerServer()) {
            return "singleplayer";
        }
        return "";
    }
}
