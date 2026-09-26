-- Trust model for wrong-solution defense.
-- New capsules are pending (success_count=0); peers confirm/reject via vote-capsule.
-- Quarantined rows are hidden from anon SELECT.

alter table public.experience_capsules
  drop constraint if exists experience_capsules_success_count_pos;

alter table public.experience_capsules
  add column if not exists status text not null default 'pending',
  add column if not exists confirm_count integer not null default 0,
  add column if not exists reject_count integer not null default 0,
  add column if not exists trust_score double precision not null default 0;

alter table public.experience_capsules
  add constraint experience_capsules_success_count_nonneg check (success_count >= 0);

alter table public.experience_capsules
  drop constraint if exists experience_capsules_status_check;

alter table public.experience_capsules
  add constraint experience_capsules_status_check
  check (status in ('pending', 'active', 'quarantined'));

-- Existing rows (if any) keep visibility; mark as active with at least their counts.
update public.experience_capsules
set status = 'active',
    trust_score = greatest(trust_score, least(1.0, success_count::float / (success_count + fail_count + 1)))
where status = 'pending' and success_count >= 1;

create index if not exists experience_capsules_status_idx
  on public.experience_capsules (status);

create index if not exists experience_capsules_fp_status_idx
  on public.experience_capsules (fingerprint_key, status);

-- One vote per (capsule, voter). Author cannot confirm own capsule.
create table if not exists public.capsule_votes (
  content_hash text not null references public.experience_capsules(content_hash) on delete cascade,
  voter_public_key text not null,
  vote text not null check (vote in ('confirm', 'reject')),
  signature text not null,
  created_at timestamptz not null default now(),
  primary key (content_hash, voter_public_key)
);

alter table public.capsule_votes enable row level security;

drop policy if exists capsule_votes_deny_all on public.capsule_votes;
create policy capsule_votes_deny_all
  on public.capsule_votes
  for all
  to anon, authenticated
  using (false)
  with check (false);

revoke all on public.capsule_votes from anon, authenticated;

-- Hide quarantined from public read. Pending + active remain visible (ranked by trust client-side).
drop policy if exists experience_capsules_select_anon on public.experience_capsules;
create policy experience_capsules_select_anon
  on public.experience_capsules
  for select
  to anon, authenticated
  using (status in ('pending', 'active'));

-- Per-fingerprint publish rate (service role only).
create table if not exists public.capsule_fp_publish_rate (
  fingerprint_key text primary key,
  window_start timestamptz not null default now(),
  publish_count integer not null default 0
);

alter table public.capsule_fp_publish_rate enable row level security;
drop policy if exists capsule_fp_publish_rate_deny_all on public.capsule_fp_publish_rate;
create policy capsule_fp_publish_rate_deny_all
  on public.capsule_fp_publish_rate
  for all
  to anon, authenticated
  using (false)
  with check (false);
revoke all on public.capsule_fp_publish_rate from anon, authenticated;
