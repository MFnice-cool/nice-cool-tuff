-- TuffSwarm ExperienceCapsule store (Supabase-first MVP).
-- Writes MUST go through the publish-capsule Edge Function (service role).
-- Anon / authenticated clients may only SELECT.

create table if not exists public.experience_capsules (
  content_hash text primary key,
  id text not null,
  fingerprint_key text not null,
  mc_major text,
  loader text,
  solution text not null,
  actions jsonb not null default '[]'::jsonb,
  success_score double precision not null default 0.5,
  success_count integer not null default 1,
  fail_count integer not null default 0,
  signer_public_key text not null,
  signature text not null,
  signer_peer_id text,
  payload jsonb not null,
  created_at timestamptz not null default now(),
  updated_at timestamptz not null default now(),
  constraint experience_capsules_solution_nonempty check (char_length(trim(solution)) > 0),
  constraint experience_capsules_fp_nonempty check (char_length(trim(fingerprint_key)) > 0),
  constraint experience_capsules_success_count_pos check (success_count >= 1)
);

create index if not exists experience_capsules_fingerprint_key_idx
  on public.experience_capsules (fingerprint_key);

create index if not exists experience_capsules_loader_mc_idx
  on public.experience_capsules (loader, mc_major);

create index if not exists experience_capsules_success_count_idx
  on public.experience_capsules (success_count desc);

-- Rate-limit windows for publish-capsule (service role only).
create table if not exists public.capsule_publish_rate (
  signer_public_key text primary key,
  window_start timestamptz not null default now(),
  publish_count integer not null default 0
);

alter table public.experience_capsules enable row level security;
alter table public.capsule_publish_rate enable row level security;

-- Public read of capsules (no raw logs in payload by ingest policy).
drop policy if exists experience_capsules_select_anon on public.experience_capsules;
create policy experience_capsules_select_anon
  on public.experience_capsules
  for select
  to anon, authenticated
  using (true);

-- No direct inserts/updates/deletes for anon/authenticated.
-- Edge Function uses service role (bypasses RLS).

drop policy if exists capsule_publish_rate_deny_all on public.capsule_publish_rate;
create policy capsule_publish_rate_deny_all
  on public.capsule_publish_rate
  for all
  to anon, authenticated
  using (false)
  with check (false);

grant select on public.experience_capsules to anon, authenticated;
revoke insert, update, delete on public.experience_capsules from anon, authenticated;
revoke all on public.capsule_publish_rate from anon, authenticated;
