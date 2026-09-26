package dev.tuffbox.jeibridge;

import com.google.gson.JsonObject;
import mezz.jei.api.runtime.IJeiRuntime;
import net.minecraft.client.Minecraft;

import java.io.BufferedReader;
import java.io.IOException;
import java.io.InputStreamReader;
import java.io.OutputStream;
import java.net.InetAddress;
import java.net.ServerSocket;
import java.net.Socket;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.nio.file.StandardCopyOption;
import java.util.concurrent.CompletableFuture;
import java.util.concurrent.TimeUnit;

final class BridgeServer implements AutoCloseable {
    private static volatile BridgeServer active;

    private final IJeiRuntime runtime;
    private final String token;
    private final Path handshakePath;
    private final ServerSocket server;
    private final Thread acceptThread;
    private volatile boolean running = true;

    private BridgeServer(IJeiRuntime runtime, String token, Path handshakePath) throws IOException {
        this.runtime = runtime;
        this.token = token;
        this.handshakePath = handshakePath;
        this.server = new ServerSocket(0, 16, InetAddress.getLoopbackAddress());
        this.acceptThread = new Thread(this::acceptLoop, "tuffbox-jei-bridge");
        this.acceptThread.setDaemon(true);
        writeHandshake();
        this.acceptThread.start();
    }

    static synchronized void start(IJeiRuntime runtime) {
        stop();
        String token = System.getProperty("tuffbox.bridge.token", "").trim();
        String handshake = System.getProperty("tuffbox.bridge.handshake", "").trim();
        if (token.length() < 24 || handshake.isEmpty()) {
            return;
        }
        try {
            active = new BridgeServer(runtime, token, Path.of(handshake));
        } catch (IOException error) {
            System.err.println("[TuffBox JEI bridge] failed to start: " + error.getMessage());
        }
    }

    static synchronized void stop() {
        BridgeServer previous = active;
        active = null;
        if (previous != null) {
            previous.close();
        }
    }

    private void acceptLoop() {
        while (running) {
            try {
                Socket socket = server.accept();
                Thread.ofVirtual().name("tuffbox-jei-request").start(() -> handle(socket));
            } catch (IOException error) {
                if (running) {
                    System.err.println("[TuffBox JEI bridge] accept failed: " + error.getMessage());
                }
            }
        }
    }

    private void handle(Socket socket) {
        try (socket;
             BufferedReader input = new BufferedReader(
                 new InputStreamReader(socket.getInputStream(), StandardCharsets.US_ASCII))) {
            socket.setSoTimeout(10_000);
            String requestLine = input.readLine();
            if (requestLine == null) {
                return;
            }
            String suppliedToken = "";
            String line;
            while ((line = input.readLine()) != null && !line.isEmpty()) {
                int colon = line.indexOf(':');
                if (colon > 0 && line.substring(0, colon).trim().equalsIgnoreCase("X-TuffBox-Token")) {
                    suppliedToken = line.substring(colon + 1).trim();
                }
            }
            String[] request = requestLine.split(" ");
            if (request.length < 2 || !"GET".equals(request[0])) {
                respond(socket, 405, jsonError("method not allowed"));
                return;
            }
            if (!constantTimeEquals(token, suppliedToken)) {
                respond(socket, 401, jsonError("unauthorized"));
                return;
            }
            String path = request[1].split("\\?", 2)[0];
            if ("/health".equals(path) || "/v1/meta".equals(path)) {
                JsonObject health = new JsonObject();
                health.addProperty("status", "ready");
                health.addProperty("protocolVersion", 1);
                health.addProperty("minecraftVersion", "1.21.1");
                health.addProperty("pid", ProcessHandle.current().pid());
                respond(socket, 200, health.toString());
                return;
            }
            if ("/v1/snapshot".equals(path)
                || "/v1/categories".equals(path)
                || "/v1/recipes".equals(path)) {
                respond(socket, 200, snapshotOnClientThread());
                return;
            }
            respond(socket, 404, jsonError("not found"));
        } catch (Exception error) {
            try {
                respond(socket, 500, jsonError(error.getMessage()));
            } catch (IOException ignored) {
            }
        }
    }

    private String snapshotOnClientThread() throws Exception {
        CompletableFuture<String> future = new CompletableFuture<>();
        Minecraft.getInstance().execute(() -> {
            try {
                future.complete(JeiSnapshotter.snapshot(runtime).toString());
            } catch (Throwable error) {
                future.completeExceptionally(error);
            }
        });
        return future.get(30, TimeUnit.SECONDS);
    }

    private void writeHandshake() throws IOException {
        Files.createDirectories(handshakePath.getParent());
        JsonObject handshake = new JsonObject();
        handshake.addProperty("protocolVersion", 1);
        handshake.addProperty("host", "127.0.0.1");
        handshake.addProperty("port", server.getLocalPort());
        handshake.addProperty("token", token);
        handshake.addProperty("pid", ProcessHandle.current().pid());
        handshake.addProperty("minecraftVersion", "1.21.1");
        Path staged = handshakePath.resolveSibling(handshakePath.getFileName() + ".tmp");
        Files.writeString(staged, handshake.toString(), StandardCharsets.UTF_8);
        try {
            Files.move(staged, handshakePath, StandardCopyOption.ATOMIC_MOVE, StandardCopyOption.REPLACE_EXISTING);
        } catch (IOException unsupportedAtomicMove) {
            Files.move(staged, handshakePath, StandardCopyOption.REPLACE_EXISTING);
        }
    }

    private static void respond(Socket socket, int status, String body) throws IOException {
        byte[] bytes = body.getBytes(StandardCharsets.UTF_8);
        String reason = switch (status) {
            case 200 -> "OK";
            case 401 -> "Unauthorized";
            case 404 -> "Not Found";
            case 405 -> "Method Not Allowed";
            default -> "Internal Server Error";
        };
        OutputStream output = socket.getOutputStream();
        output.write(("HTTP/1.1 " + status + " " + reason + "\r\n"
            + "Content-Type: application/json; charset=utf-8\r\n"
            + "Cache-Control: no-store\r\n"
            + "Connection: close\r\n"
            + "Content-Length: " + bytes.length + "\r\n\r\n").getBytes(StandardCharsets.US_ASCII));
        output.write(bytes);
        output.flush();
    }

    private static String jsonError(String message) {
        JsonObject error = new JsonObject();
        error.addProperty("error", message == null ? "unknown error" : message);
        return error.toString();
    }

    private static boolean constantTimeEquals(String expected, String actual) {
        byte[] left = expected.getBytes(StandardCharsets.UTF_8);
        byte[] right = actual.getBytes(StandardCharsets.UTF_8);
        int difference = left.length ^ right.length;
        for (int i = 0; i < Math.max(left.length, right.length); i++) {
            byte rightByte = right.length == 0 ? 0 : right[i % right.length];
            difference |= left[i % left.length] ^ rightByte;
        }
        return difference == 0;
    }

    @Override
    public void close() {
        running = false;
        try {
            server.close();
        } catch (IOException ignored) {
        }
        try {
            Files.deleteIfExists(handshakePath);
        } catch (IOException ignored) {
        }
    }
}
