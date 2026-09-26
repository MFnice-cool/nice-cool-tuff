package dev.tuffbox.cosmetics;

import com.mojang.blaze3d.vertex.PoseStack;
import com.mojang.blaze3d.vertex.VertexConsumer;
import net.fabricmc.fabric.api.event.player.AttackEntityCallback;
import net.fabricmc.fabric.api.client.rendering.v1.WorldRenderContext;
import net.fabricmc.fabric.api.client.rendering.v1.WorldRenderEvents;
import net.minecraft.client.Minecraft;
import net.minecraft.client.renderer.MultiBufferSource;
import net.minecraft.core.particles.ParticleTypes;
import net.minecraft.util.Mth;
import net.minecraft.world.InteractionHand;
import net.minecraft.world.InteractionResult;
import net.minecraft.world.entity.Entity;
import net.minecraft.world.entity.LivingEntity;
import net.minecraft.world.entity.player.Player;
import net.minecraft.world.level.Level;
import net.minecraft.world.phys.AABB;
import net.minecraft.world.phys.EntityHitResult;
import net.minecraft.world.phys.Vec3;

import java.util.ArrayList;
import java.util.Iterator;
import java.util.List;
import java.util.UUID;

/**
 * Combat visuals: hit particles / bubbles, kill burst, target ESP.
 * Feature surface inspired by common PvP clients; geometry is original.
 * Do not copy Soup (ARR) or Meteor (GPL) source.
 */
public final class CombatFx {
    private static final int FULLBRIGHT = 0xF000F0;

    private static final List<Burst> BURSTS = new ArrayList<>();
    private static final List<Bubble> BUBBLES = new ArrayList<>();
    private static final List<Shard> SHARDS = new ArrayList<>();

    private static UUID lastHitId;
    private static float lastHitHp = -1f;
    private static int lastHitAge;

    private CombatFx() {}

    public static void init() {
        AttackEntityCallback.EVENT.register(CombatFx::onAttack);
        WorldRenderEvents.AFTER_ENTITIES.register(CombatFx::render);
    }

    private static InteractionResult onAttack(
            Player player, Level level, InteractionHand hand, Entity target, EntityHitResult hit
    ) {
        if (!level.isClientSide() || !(player instanceof net.minecraft.client.player.LocalPlayer)) {
            return InteractionResult.PASS;
        }
        Minecraft mc = Minecraft.getInstance();
        if (mc.player == null || player != mc.player) return InteractionResult.PASS;

        Vec3 at = hit != null ? hit.getLocation() : target.position().add(0, target.getBbHeight() * 0.5, 0);

        if (CosmeticsProfiles.hitParticlesFor(player)) {
            spawnHitParticles(mc, at);
        }
        if (CosmeticsProfiles.hitBubblesFor(player)) {
            BUBBLES.add(new Bubble(at.x, at.y, at.z, 16, 0.2f, 0.85f));
            BUBBLES.add(new Bubble(at.x, at.y, at.z, 12, 0.1f, 0.55f));
        }

        if (target instanceof LivingEntity living) {
            lastHitId = living.getUUID();
            lastHitHp = living.getHealth();
            lastHitAge = 0;
        }
        return InteractionResult.PASS;
    }

    private static void spawnHitParticles(Minecraft mc, Vec3 at) {
        if (mc.level == null) return;
        for (int i = 0; i < 10; i++) {
            double vx = (Math.random() - 0.5) * 0.35;
            double vy = Math.random() * 0.35;
            double vz = (Math.random() - 0.5) * 0.35;
            mc.level.addParticle(ParticleTypes.CRIT, at.x, at.y, at.z, vx, vy, vz);
            if (i % 2 == 0) {
                mc.level.addParticle(ParticleTypes.ENCHANTED_HIT, at.x, at.y, at.z, vx * 0.5, vy, vz * 0.5);
            }
        }
        for (int i = 0; i < 18; i++) {
            float yaw = (float) (Math.random() * Math.PI * 2);
            float pitch = (float) (Math.random() * Math.PI - Math.PI / 2);
            float speed = 0.08f + (float) Math.random() * 0.18f;
            SHARDS.add(new Shard(
                    at.x, at.y, at.z,
                    Mth.cos(yaw) * Mth.cos(pitch) * speed,
                    Mth.sin(pitch) * speed,
                    Mth.sin(yaw) * Mth.cos(pitch) * speed,
                    14 + (int) (Math.random() * 10),
                    (float) Math.random()
            ));
        }
    }

    public static void tick(Minecraft client) {
        if (client.level == null || client.player == null || client.isPaused()) return;

        // kill detect after our hit
        if (lastHitId != null) {
            lastHitAge++;
            Entity e = null;
            AABB box = client.player.getBoundingBox().inflate(12);
            for (LivingEntity le : client.level.getEntitiesOfClass(LivingEntity.class, box)) {
                if (le.getUUID().equals(lastHitId)) {
                    e = le;
                    break;
                }
            }
            boolean dead = e == null
                    || (e instanceof LivingEntity le && (le.isDeadOrDying() || le.getHealth() <= 0f));
            if (dead && CosmeticsProfiles.killEffectFor(client.player)) {
                Vec3 p = e != null ? e.position().add(0, e.getBbHeight() * 0.5, 0) : client.player.position();
                spawnKillBurst(p);
                lastHitId = null;
            } else if (lastHitAge > 40) {
                lastHitId = null;
            } else if (e instanceof LivingEntity le && le.getHealth() < lastHitHp) {
                lastHitHp = le.getHealth();
            }
        }

        BURSTS.removeIf(b -> ++b.age > b.maxAge);
        BUBBLES.removeIf(b -> ++b.age > b.maxAge);
        Iterator<Shard> it = SHARDS.iterator();
        while (it.hasNext()) {
            Shard s = it.next();
            s.x += s.vx;
            s.y += s.vy;
            s.z += s.vz;
            s.vy -= 0.012;
            if (++s.age > s.maxAge) it.remove();
        }
    }

    private static void spawnKillBurst(Vec3 at) {
        BURSTS.add(new Burst(at.x, at.y, at.z, 28, 0.3f, 2.4f));
        for (int i = 0; i < 3; i++) {
            BUBBLES.add(new Bubble(at.x, at.y, at.z, 20 - i * 3, 0.15f + i * 0.1f, 1.2f + i * 0.4f));
        }
        Minecraft mc = Minecraft.getInstance();
        if (mc.level != null) {
            for (int i = 0; i < 24; i++) {
                mc.level.addParticle(
                        ParticleTypes.FIREWORK,
                        at.x, at.y, at.z,
                        (Math.random() - 0.5) * 0.4,
                        Math.random() * 0.5,
                        (Math.random() - 0.5) * 0.4
                );
            }
        }
    }

    private static void render(WorldRenderContext ctx) {
        Minecraft mc = Minecraft.getInstance();
        if (mc.player == null) return;

        MultiBufferSource.BufferSource buffers = mc.renderBuffers().bufferSource();
        PoseStack pose = ctx.matrixStack();
        Vec3 cam = ctx.camera().getPosition();
        float partial = ctx.tickCounter().getGameTimeDeltaPartialTick(false);
        VertexConsumer vc = buffers.getBuffer(CosmeticsRender.emissive());

        // Target ESP — box around crosshair entity
        if (CosmeticsProfiles.targetEspFor(mc.player)
                && mc.hitResult instanceof EntityHitResult ehr
                && ehr.getEntity() instanceof LivingEntity target
                && target.isAlive()) {
            renderTargetBox(pose, vc, cam, target, partial, mc.player.tickCount + partial);
        }

        for (Burst b : BURSTS) {
            float t = (b.age + partial) / b.maxAge;
            if (t > 1f) continue;
            float r = b.r0 + t * (b.r1 - b.r0);
            int alpha = (int) ((1f - t) * 180);
            pose.pushPose();
            pose.translate(b.x - cam.x, b.y - cam.y, b.z - cam.z);
            drawRainbowSphere(pose, vc, r, 12, 10, alpha, t);
            pose.popPose();
        }

        for (Bubble b : BUBBLES) {
            float t = (b.age + partial) / b.maxAge;
            if (t > 1f) continue;
            float r = b.r0 + t * (b.r1 - b.r0);
            int alpha = (int) ((1f - t) * 200);
            float hue = (t * 0.7f) % 1f;
            pose.pushPose();
            pose.translate(b.x - cam.x, b.y - cam.y, b.z - cam.z);
            // horizontal + vertical rings = “bubble”
            drawRingFlat(pose, vc, 0, r, r * 0.88f, 36, CosmeticsRender.hsva(hue, 0.85f, 1f, alpha));
            pose.mulPose(com.mojang.math.Axis.XP.rotationDegrees(90));
            drawRingFlat(pose, vc, 0, r * 0.95f, r * 0.82f, 28, CosmeticsRender.hsva(hue + 0.15f, 0.8f, 1f, alpha * 3 / 4));
            pose.popPose();
        }

        for (Shard s : SHARDS) {
            float life = 1f - (s.age + partial) / s.maxAge;
            if (life <= 0) continue;
            int col = CosmeticsRender.hsva(s.hue, 0.9f, 1f, (int) (life * 220));
            float sz = 0.04f + life * 0.03f;
            pose.pushPose();
            pose.translate(s.x - cam.x, s.y - cam.y, s.z - cam.z);
            PoseStack.Pose last = pose.last();
            CosmeticsRender.vert(vc, last, FULLBRIGHT, -sz, 0, -sz, col);
            CosmeticsRender.vert(vc, last, FULLBRIGHT, sz, 0, -sz, col);
            CosmeticsRender.vert(vc, last, FULLBRIGHT, sz, 0, sz, col);
            CosmeticsRender.vert(vc, last, FULLBRIGHT, -sz, 0, sz, col);
            pose.popPose();
        }

        buffers.endBatch();
    }

    private static void renderTargetBox(
            PoseStack pose, VertexConsumer vc, Vec3 cam, LivingEntity target, float partial, float time
    ) {
        AABB bb = target.getBoundingBox().inflate(0.05);
        double x0 = bb.minX - cam.x;
        double y0 = bb.minY - cam.y;
        double z0 = bb.minZ - cam.z;
        double x1 = bb.maxX - cam.x;
        double y1 = bb.maxY - cam.y;
        double z1 = bb.maxZ - cam.z;

        float hue = (time * 0.02f) % 1f;
        int edge = CosmeticsRender.hsva(hue, 0.9f, 1f, 200);
        int fill = CosmeticsRender.hsva(hue, 0.5f, 1f, 35);

        // soft fill
        drawBoxQuads(pose, vc, x0, y0, z0, x1, y1, z1, fill);
        // edge ribbons (thick lines as thin boxes)
        float w = 0.02f;
        drawEdge(pose, vc, x0, y0, z0, x1, y0, z0, w, edge);
        drawEdge(pose, vc, x0, y0, z1, x1, y0, z1, w, edge);
        drawEdge(pose, vc, x0, y1, z0, x1, y1, z0, w, edge);
        drawEdge(pose, vc, x0, y1, z1, x1, y1, z1, w, edge);
        drawEdge(pose, vc, x0, y0, z0, x0, y0, z1, w, edge);
        drawEdge(pose, vc, x1, y0, z0, x1, y0, z1, w, edge);
        drawEdge(pose, vc, x0, y1, z0, x0, y1, z1, w, edge);
        drawEdge(pose, vc, x1, y1, z0, x1, y1, z1, w, edge);
        drawEdge(pose, vc, x0, y0, z0, x0, y1, z0, w, edge);
        drawEdge(pose, vc, x1, y0, z0, x1, y1, z0, w, edge);
        drawEdge(pose, vc, x0, y0, z1, x0, y1, z1, w, edge);
        drawEdge(pose, vc, x1, y0, z1, x1, y1, z1, w, edge);

        // ground circle under target
        pose.pushPose();
        pose.translate(
                target.xo + (target.getX() - target.xo) * partial - cam.x,
                bb.minY - cam.y + 0.02,
                target.zo + (target.getZ() - target.zo) * partial - cam.z
        );
        float rad = target.getBbWidth() * 0.85f;
        drawRingFlat(pose, vc, 0, rad, rad * 0.82f, 40, CosmeticsRender.hsva(hue + 0.3f, 0.85f, 1f, 160));
        pose.popPose();
    }

    private static void drawBoxQuads(
            PoseStack pose, VertexConsumer vc,
            double x0, double y0, double z0, double x1, double y1, double z1, int col
    ) {
        PoseStack.Pose last = pose.last();
        float a = (float) x0, b = (float) y0, c = (float) z0;
        float d = (float) x1, e = (float) y1, f = (float) z1;
        // only bottom + top to keep fill light
        CosmeticsRender.vert(vc, last, FULLBRIGHT, a, b, c, col);
        CosmeticsRender.vert(vc, last, FULLBRIGHT, d, b, c, col);
        CosmeticsRender.vert(vc, last, FULLBRIGHT, d, b, f, col);
        CosmeticsRender.vert(vc, last, FULLBRIGHT, a, b, f, col);
        CosmeticsRender.vert(vc, last, FULLBRIGHT, a, e, c, col);
        CosmeticsRender.vert(vc, last, FULLBRIGHT, a, e, f, col);
        CosmeticsRender.vert(vc, last, FULLBRIGHT, d, e, f, col);
        CosmeticsRender.vert(vc, last, FULLBRIGHT, d, e, c, col);
    }

    private static void drawEdge(
            PoseStack pose, VertexConsumer vc,
            double x0, double y0, double z0, double x1, double y1, double z1, float w, int col
    ) {
        PoseStack.Pose last = pose.last();
        float ax = (float) x0, ay = (float) y0, az = (float) z0;
        float bx = (float) x1, by = (float) y1, bz = (float) z1;
        CosmeticsRender.vert(vc, last, FULLBRIGHT, ax - w, ay, az - w, col);
        CosmeticsRender.vert(vc, last, FULLBRIGHT, bx - w, by, bz - w, col);
        CosmeticsRender.vert(vc, last, FULLBRIGHT, bx + w, by, bz + w, col);
        CosmeticsRender.vert(vc, last, FULLBRIGHT, ax + w, ay, az + w, col);
    }

    private static void drawRingFlat(
            PoseStack pose, VertexConsumer vc, float y, float ro, float ri, int segs, int col
    ) {
        PoseStack.Pose last = pose.last();
        for (int i = 0; i < segs; i++) {
            float a0 = (float) (i * Math.PI * 2 / segs);
            float a1 = (float) ((i + 1) * Math.PI * 2 / segs);
            float c0 = Mth.cos(a0), s0 = Mth.sin(a0);
            float c1 = Mth.cos(a1), s1 = Mth.sin(a1);
            CosmeticsRender.vert(vc, last, FULLBRIGHT, c0 * ro, y, s0 * ro, col);
            CosmeticsRender.vert(vc, last, FULLBRIGHT, c1 * ro, y, s1 * ro, col);
            CosmeticsRender.vert(vc, last, FULLBRIGHT, c1 * ri, y, s1 * ri, col);
            CosmeticsRender.vert(vc, last, FULLBRIGHT, c0 * ri, y, s0 * ri, col);
        }
    }

    private static void drawRainbowSphere(
            PoseStack pose, VertexConsumer vc, float r, int slices, int stacks, int alpha, float hueOff
    ) {
        PoseStack.Pose last = pose.last();
        for (int i = 0; i < stacks; i++) {
            float v0 = (float) i / stacks;
            float v1 = (float) (i + 1) / stacks;
            float phi0 = (float) (Math.PI * (v0 - 0.5));
            float phi1 = (float) (Math.PI * (v1 - 0.5));
            float y0 = Mth.sin(phi0) * r;
            float y1 = Mth.sin(phi1) * r;
            float r0 = Mth.cos(phi0) * r;
            float r1 = Mth.cos(phi1) * r;
            for (int j = 0; j < slices; j++) {
                float u0 = (float) j / slices;
                float u1 = (float) (j + 1) / slices;
                float th0 = (float) (u0 * Math.PI * 2);
                float th1 = (float) (u1 * Math.PI * 2);
                int col = CosmeticsRender.hsva((hueOff + u0) % 1f, 0.85f, 1f, alpha / (1 + i / 4));
                CosmeticsRender.vert(vc, last, FULLBRIGHT, Mth.cos(th0) * r0, y0, Mth.sin(th0) * r0, col);
                CosmeticsRender.vert(vc, last, FULLBRIGHT, Mth.cos(th1) * r0, y0, Mth.sin(th1) * r0, col);
                CosmeticsRender.vert(vc, last, FULLBRIGHT, Mth.cos(th1) * r1, y1, Mth.sin(th1) * r1, col);
                CosmeticsRender.vert(vc, last, FULLBRIGHT, Mth.cos(th0) * r1, y1, Mth.sin(th0) * r1, col);
            }
        }
    }

    private static final class Burst {
        final double x, y, z;
        final int maxAge;
        final float r0, r1;
        int age;

        Burst(double x, double y, double z, int maxAge, float r0, float r1) {
            this.x = x;
            this.y = y;
            this.z = z;
            this.maxAge = maxAge;
            this.r0 = r0;
            this.r1 = r1;
        }
    }

    private static final class Bubble {
        final double x, y, z;
        final int maxAge;
        final float r0, r1;
        int age;

        Bubble(double x, double y, double z, int maxAge, float r0, float r1) {
            this.x = x;
            this.y = y;
            this.z = z;
            this.maxAge = maxAge;
            this.r0 = r0;
            this.r1 = r1;
        }
    }

    private static final class Shard {
        double x, y, z;
        final double vx;
        double vy;
        final double vz;
        final int maxAge;
        final float hue;
        int age;

        Shard(double x, double y, double z, double vx, double vy, double vz, int maxAge, float hue) {
            this.x = x;
            this.y = y;
            this.z = z;
            this.vx = vx;
            this.vy = vy;
            this.vz = vz;
            this.maxAge = maxAge;
            this.hue = hue;
        }
    }
}
