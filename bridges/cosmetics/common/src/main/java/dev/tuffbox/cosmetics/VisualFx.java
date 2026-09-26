package dev.tuffbox.cosmetics;

import com.mojang.blaze3d.vertex.PoseStack;
import com.mojang.blaze3d.vertex.VertexConsumer;
import net.fabricmc.fabric.api.client.rendering.v1.WorldRenderContext;
import net.fabricmc.fabric.api.client.rendering.v1.WorldRenderEvents;
import net.minecraft.client.Minecraft;
import net.minecraft.client.renderer.MultiBufferSource;
import net.minecraft.util.Mth;
import net.minecraft.world.entity.player.Player;
import net.minecraft.world.phys.Vec3;

import java.util.ArrayList;
import java.util.HashMap;
import java.util.Iterator;
import java.util.List;
import java.util.Map;
import java.util.UUID;

/**
 * Jump circles + movement trails for local player and nearby peers with cosmetics flags.
 * Original FX — Soup Visuals is inspiration-only (do not copy their code).
 */
public final class VisualFx {
    private static final int FULLBRIGHT = 0xF000F0;
    private static final int TRAIL_MAX = 48;
    private static final int TRAIL_LIFE = 28;

    private static final List<Circle> CIRCLES = new ArrayList<>();
    private static final Map<UUID, PlayerTrail> TRAILS = new HashMap<>();
    private static final Map<UUID, Boolean> WAS_ON_GROUND = new HashMap<>();

    private VisualFx() {}

    public static void init() {
        WorldRenderEvents.AFTER_ENTITIES.register(VisualFx::render);
    }

    public static void tick(Minecraft client) {
        if (client.level == null || client.player == null) return;
        if (client.isPaused()) return;

        for (Player player : client.level.players()) {
            UUID id = player.getUUID();
            boolean jumpOn = CosmeticsProfiles.jumpCirclesFor(player);
            boolean trailOn = CosmeticsProfiles.trailFor(player);

            if (jumpOn) {
                boolean onGround = player.onGround();
                Boolean was = WAS_ON_GROUND.get(id);
                if (was != null && was && !onGround && player.getDeltaMovement().y > 0.05) {
                    spawnJumpBurst(player);
                }
                // landing ring
                if (was != null && !was && onGround && player.fallDistance > 0.4f) {
                    spawnLandBurst(player);
                }
                WAS_ON_GROUND.put(id, onGround);
            } else {
                WAS_ON_GROUND.remove(id);
            }

            // sprint sparks along trail
            if (trailOn && player.isSprinting() && player.onGround()) {
                Vec3 p = player.position();
                if ((player.tickCount + id.hashCode()) % 3 == 0) {
                    CIRCLES.add(new Circle(
                            p.x + (Math.random() - 0.5) * 0.3,
                            p.y + 0.05,
                            p.z + (Math.random() - 0.5) * 0.3,
                            8, 0.05f, 0.35f, (float) Math.random()));
                }
            }

            if (trailOn) {
                Vec3 vel = player.getDeltaMovement();
                double speed = vel.horizontalDistance();
                if (speed > 0.02 || !player.onGround()) {
                    PlayerTrail trail = TRAILS.computeIfAbsent(id, u -> new PlayerTrail());
                    // throttle: keep density readable
                    if (trail.points.isEmpty() || trail.points.get(trail.points.size() - 1).distTo(player) > 0.18) {
                        double y = player.getY() + 0.08;
                        trail.points.add(new TrailPoint(player.getX(), y, player.getZ(), TRAIL_LIFE));
                        while (trail.points.size() > TRAIL_MAX) {
                            trail.points.remove(0);
                        }
                    }
                }
            } else {
                TRAILS.remove(id);
            }
        }

        // age trails / drop idle players
        Iterator<Map.Entry<UUID, PlayerTrail>> it = TRAILS.entrySet().iterator();
        while (it.hasNext()) {
            Map.Entry<UUID, PlayerTrail> e = it.next();
            PlayerTrail trail = e.getValue();
            trail.points.removeIf(p -> --p.life <= 0);
            if (trail.points.isEmpty()) it.remove();
        }

        CIRCLES.removeIf(c -> {
            c.age++;
            return c.age > c.maxAge;
        });
    }

    private static void spawnJumpBurst(Player player) {
        Vec3 p = player.position();
        // three expanding rings with phase offsets
        CIRCLES.add(new Circle(p.x, p.y + 0.04, p.z, 22, 0.25f, 1.8f, 0f));
        CIRCLES.add(new Circle(p.x, p.y + 0.05, p.z, 18, 0.15f, 1.2f, 0.12f));
        CIRCLES.add(new Circle(p.x, p.y + 0.06, p.z, 14, 0.08f, 0.7f, 0.25f));
    }

    private static void spawnLandBurst(Player player) {
        Vec3 p = player.position();
        CIRCLES.add(new Circle(p.x, p.y + 0.03, p.z, 14, 0.4f, 1.5f, 0.5f));
        CIRCLES.add(new Circle(p.x, p.y + 0.04, p.z, 10, 0.2f, 0.9f, 0.65f));
    }

    private static void render(WorldRenderContext ctx) {
        if (CIRCLES.isEmpty() && TRAILS.isEmpty()) return;

        MultiBufferSource.BufferSource buffers = Minecraft.getInstance().renderBuffers().bufferSource();
        PoseStack pose = ctx.matrixStack();
        Vec3 cam = ctx.camera().getPosition();
        float partial = ctx.tickCounter().getGameTimeDeltaPartialTick(false);
        VertexConsumer vc = buffers.getBuffer(CosmeticsRender.emissive());

        for (Circle c : CIRCLES) {
            float t = (c.age + partial) / c.maxAge;
            if (t > 1f) continue;
            float ease = 1f - (1f - t) * (1f - t);
            float radius = c.r0 + ease * (c.r1 - c.r0);
            float width = 0.06f + (1f - t) * 0.05f;
            int alpha = (int) ((1f - t) * 200);
            float hue = (t * 0.6f + c.huePhase) % 1f;

            pose.pushPose();
            pose.translate(c.x - cam.x, c.y - cam.y, c.z - cam.z);
            // rainbow segments
            PoseStack.Pose last = pose.last();
            int segs = 48;
            for (int i = 0; i < segs; i++) {
                float a0 = (float) (i * Math.PI * 2 / segs);
                float a1 = (float) ((i + 1) * Math.PI * 2 / segs);
                float h = (hue + i / (float) segs) % 1f;
                int col = CosmeticsRender.hsva(h, 0.85f, 1f, alpha);
                float c0 = Mth.cos(a0);
                float s0 = Mth.sin(a0);
                float c1 = Mth.cos(a1);
                float s1 = Mth.sin(a1);
                float ri = radius - width;
                float ro = radius + width * 0.35f;
                CosmeticsRender.vert(vc, last, FULLBRIGHT, c0 * ro, 0, s0 * ro, col);
                CosmeticsRender.vert(vc, last, FULLBRIGHT, c1 * ro, 0, s1 * ro, col);
                CosmeticsRender.vert(vc, last, FULLBRIGHT, c1 * ri, 0, s1 * ri, col);
                CosmeticsRender.vert(vc, last, FULLBRIGHT, c0 * ri, 0, s0 * ri, col);
            }
            // soft fill disc (early life only)
            if (t < 0.35f) {
                int fillA = (int) ((0.35f - t) / 0.35f * 40);
                int fill = CosmeticsRender.hsva(hue, 0.4f, 1f, fillA);
                CosmeticsRender.ring(pose, vc, FULLBRIGHT, 0.001f, 0f, radius * 0.85f, 24, fill);
            }
            pose.popPose();
        }

        for (PlayerTrail trail : TRAILS.values()) {
            List<TrailPoint> pts = trail.points;
            if (pts.size() < 2) continue;
            for (int i = 1; i < pts.size(); i++) {
                TrailPoint a = pts.get(i - 1);
                TrailPoint b = pts.get(i);
                float life = (b.life - partial) / (float) TRAIL_LIFE;
                if (life <= 0) continue;
                float hue = (i / (float) pts.size() + life * 0.3f) % 1f;
                int alpha = (int) (life * 180);
                int col = CosmeticsRender.hsva(hue, 0.75f, 1f, alpha);
                float half = 0.04f + life * 0.05f;

                pose.pushPose();
                // ribbon in XZ plane following segment
                double mx = (a.x + b.x) * 0.5 - cam.x;
                double my = (a.y + b.y) * 0.5 - cam.y;
                double mz = (a.z + b.z) * 0.5 - cam.z;
                pose.translate(mx, my, mz);
                double dx = b.x - a.x;
                double dz = b.z - a.z;
                double len = Math.sqrt(dx * dx + dz * dz);
                if (len < 1e-4) {
                    pose.popPose();
                    continue;
                }
                float yaw = (float) Mth.atan2(dz, dx);
                pose.mulPose(com.mojang.math.Axis.YP.rotation((float) (-yaw + Math.PI / 2)));
                PoseStack.Pose last = pose.last();
                float hl = (float) (len * 0.5);
                CosmeticsRender.vert(vc, last, FULLBRIGHT, -half, 0, -hl, col);
                CosmeticsRender.vert(vc, last, FULLBRIGHT, half, 0, -hl, col);
                CosmeticsRender.vert(vc, last, FULLBRIGHT, half, 0, hl, col);
                CosmeticsRender.vert(vc, last, FULLBRIGHT, -half, 0, hl, col);
                pose.popPose();
            }
        }

        buffers.endBatch();
    }

    private static final class Circle {
        final double x, y, z;
        final int maxAge;
        final float r0, r1, huePhase;
        int age;

        Circle(double x, double y, double z, int maxAge, float r0, float r1, float huePhase) {
            this.x = x;
            this.y = y;
            this.z = z;
            this.maxAge = maxAge;
            this.r0 = r0;
            this.r1 = r1;
            this.huePhase = huePhase;
        }
    }

    private static final class TrailPoint {
        final double x, y, z;
        int life;

        TrailPoint(double x, double y, double z, int life) {
            this.x = x;
            this.y = y;
            this.z = z;
            this.life = life;
        }

        double distTo(Player p) {
            double dx = p.getX() - x;
            double dy = p.getY() + 0.08 - y;
            double dz = p.getZ() - z;
            return Math.sqrt(dx * dx + dy * dy + dz * dz);
        }
    }

    private static final class PlayerTrail {
        final List<TrailPoint> points = new ArrayList<>();
    }
}
