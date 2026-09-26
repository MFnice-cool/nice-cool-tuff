# TuffSwarm Supabase backend

Preferred start transport for signed `ExperienceCapsule` exchange.

## Deploy

```bash
# From repo root, with Supabase CLI linked to your project:
supabase db push
supabase functions deploy publish-capsule --no-verify-jwt
supabase functions deploy vote-capsule
supabase functions deploy report-cooccurrence --no-verify-jwt
supabase functions deploy fetch-youtube-feed --no-verify-jwt
supabase functions deploy cosmetics-get --no-verify-jwt
supabase functions deploy cosmetics-upsert --no-verify-jwt
```

`vote-capsule` must keep JWT verification on (Auth login required to vote).
`report-cooccurrence` is anon-callable (rate-limited; service role inside).
`fetch-youtube-feed` is cron/service only (`verify_jwt=false`); needs secret `YOUTUBE-API-KEY` (also accepts `YOUTUBE_API_KEY`).
`cosmetics-get` / `cosmetics-upsert` are anon-callable (`verify_jwt=false`); upsert ownership via `writeSecret` hash. See [`docs/14-cosmetics.md`](../docs/14-cosmetics.md).

### YouTube home feed

1. Apply migrations `011_youtube_feed.sql` + `012_youtube_feed_lang_source.sql` (`source`, `lang`).
2. Set secret: `supabase secrets set YOUTUBE-API-KEY=<google-youtube-data-api-v3-key>`
3. Deploy: `supabase functions deploy fetch-youtube-feed --no-verify-jwt`
4. Schedule: pg_cron job `fetch-youtube-feed-every-2h` (`0 */2 * * *`) posts to the Edge Function (or Dashboard Schedules).
5. First fill: `supabase functions invoke fetch-youtube-feed` (or HTTP POST to `/functions/v1/fetch-youtube-feed`).

Launcher home reads `youtube_feed` via PostgREST (thumbnails only; click opens system browser). Clients never call YouTube.
Client diversifies the strip: channel caps, popular↔creator interleave, freshness + soft daily rotation; filters to the OS UI language **or English**.

Tracked sources: per-locale keyword searches (en/ru/uk/de/es/fr/pt/pl) plus channel uploads for Dr Donut, Dream, Carvs, Kaisora, JudeLow.

## Client settings

TuffBox ships with the community Supabase URL + publishable key built in.
Users only enable **Use TuffSwarm network** — no keys to paste.

Optional Advanced override in Settings for self-hosted projects.

**Never** put the service role or personal access token in the client.

## Security model

| Layer | Behavior |
|-------|----------|
| RLS | `experience_capsules`: SELECT for anon/authenticated on `open`/`saved` only; `rejected` hidden |
| RLS | `mod_cooccurrence_pairs`: SELECT for anon/authenticated; writes via Edge Function only |
| RLS | `youtube_feed`: SELECT for anon/authenticated; writes via `fetch-youtube-feed` only |
| Write path | Edge Function `publish-capsule` with service role (`verify_jwt=false`; Ed25519 soft-sign) |
| Co-occurrence | Edge Function `report-cooccurrence` expands mod sets → pair counts (`bump_mod_cooccurrence_pairs`) |
| Capsule | Must include `contentHash` + Ed25519 signature + ≥1 valid action |
| Client counters | **Ignored** — new capsules start `open`, `success_count=0` |
| Peer votes | `vote-capsule` requires Supabase Auth JWT (`verify_jwt=true`); one vote per user; 2 Keep → `saved`; 3 Discard → `rejected` |
| Crash Votes UI | Register / sign in required before Keep/Discard |
| Rate limits | Per signer / hour, per fingerprint / day, open-per-signer+fp cap; co-occurrence 30 reports/hour/device |
| Privacy | Reject notes / raw crash logs |

**Never** put the service role or personal access token in the client.
