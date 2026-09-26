-- Kudos / RAC ledger (off-chain, BOINC-style).
-- Reward authors (device signer_public_key) when a peer soft-verifies confirm via vote-capsule.
-- NOT for Fog L2 replies. No on-chain. Service-role write only.

create table if not exists public.kudos_balances (
  beneficiary_key text primary key,
  total_kudos numeric(14, 2) not null default 0 check (total_kudos >= 0),
  rac numeric(14, 4) not null default 0 check (rac >= 0),
  rac_updated_at timestamptz not null default now(),
  updated_at timestamptz not null default now(),
  constraint kudos_balances_key_nonempty check (char_length(trim(beneficiary_key)) > 0)
);

comment on table public.kudos_balances is
  'Per capsule-author (Ed25519 signer_public_key) Kudos total + Recent Average Credit.';

create table if not exists public.kudos_ledger (
  id bigint generated always as identity primary key,
  beneficiary_key text not null,
  amount numeric(12, 2) not null check (amount > 0),
  reason text not null check (reason in ('soft_verify_confirm')),
  content_hash text not null references public.experience_capsules (content_hash) on delete cascade,
  voter_user_id uuid not null references auth.users (id) on delete cascade,
  created_at timestamptz not null default now(),
  constraint kudos_ledger_once unique (content_hash, voter_user_id, reason)
);

create index if not exists kudos_ledger_beneficiary_idx
  on public.kudos_ledger (beneficiary_key, created_at desc);
create index if not exists kudos_ledger_hash_idx
  on public.kudos_ledger (content_hash);

alter table public.kudos_balances enable row level security;
alter table public.kudos_ledger enable row level security;

-- Public read of reputation totals (signer keys are already on capsules).
drop policy if exists kudos_balances_select_public on public.kudos_balances;
create policy kudos_balances_select_public
  on public.kudos_balances
  for select
  to anon, authenticated
  using (true);

-- Ledger is audit-only; no client read/write (service role bypasses RLS).
drop policy if exists kudos_ledger_deny_all on public.kudos_ledger;
create policy kudos_ledger_deny_all
  on public.kudos_ledger
  for all
  to anon, authenticated
  using (false)
  with check (false);

revoke all on public.kudos_ledger from anon, authenticated;
grant select on public.kudos_balances to anon, authenticated, service_role;
grant all on public.kudos_balances to service_role;
grant all on public.kudos_ledger to service_role;
grant usage, select on sequence public.kudos_ledger_id_seq to service_role;

-- BOINC-style RAC: half-life 7 days. Discrete award:
--   rac := rac * 0.5^(days/7) + amount
create or replace function public.kudos_decay_rac(
  p_rac numeric,
  p_rac_updated_at timestamptz,
  p_now timestamptz default now()
)
returns numeric
language sql
immutable
set search_path = public, pg_temp
as $$
  select greatest(
    0::numeric,
    coalesce(p_rac, 0) * power(
      0.5::numeric,
      greatest(0::numeric, extract(epoch from (p_now - coalesce(p_rac_updated_at, p_now))) / 86400.0 / 7.0)
    )
  );
$$;

revoke all on function public.kudos_decay_rac(numeric, timestamptz, timestamptz) from public;
grant execute on function public.kudos_decay_rac(numeric, timestamptz, timestamptz)
  to anon, authenticated, service_role;

-- Award after soft-verify confirm vote. Idempotent per (hash, voter, reason).
create or replace function public.kudos_award_soft_verify(
  p_beneficiary_key text,
  p_content_hash text,
  p_voter_user_id uuid,
  p_amount numeric default 10
)
returns jsonb
language plpgsql
security definer
set search_path = public, pg_temp
as $$
declare
  v_key text := trim(p_beneficiary_key);
  v_hash text := trim(p_content_hash);
  v_amount numeric := greatest(0.01, coalesce(p_amount, 10));
  v_now timestamptz := now();
  v_total numeric;
  v_rac numeric;
  v_prev_rac numeric;
  v_prev_at timestamptz;
begin
  if v_key = '' or v_hash = '' or p_voter_user_id is null then
    return jsonb_build_object('ok', false, 'awarded', false, 'reason', 'missing_args');
  end if;

  insert into public.kudos_ledger (beneficiary_key, amount, reason, content_hash, voter_user_id)
  values (v_key, v_amount, 'soft_verify_confirm', v_hash, p_voter_user_id)
  on conflict (content_hash, voter_user_id, reason) do nothing;

  if not found then
    -- already awarded for this voter+capsule
    select total_kudos, rac into v_total, v_rac
    from public.kudos_balances
    where beneficiary_key = v_key;
    return jsonb_build_object(
      'ok', true,
      'awarded', false,
      'reason', 'already_awarded',
      'beneficiaryKey', v_key,
      'totalKudos', coalesce(v_total, 0),
      'rac', coalesce(v_rac, 0)
    );
  end if;

  select rac, rac_updated_at into v_prev_rac, v_prev_at
  from public.kudos_balances
  where beneficiary_key = v_key;

  v_rac := public.kudos_decay_rac(coalesce(v_prev_rac, 0), v_prev_at, v_now) + v_amount;

  insert into public.kudos_balances as b (beneficiary_key, total_kudos, rac, rac_updated_at, updated_at)
  values (v_key, v_amount, v_rac, v_now, v_now)
  on conflict (beneficiary_key) do update
  set
    total_kudos = b.total_kudos + v_amount,
    rac = public.kudos_decay_rac(b.rac, b.rac_updated_at, v_now) + v_amount,
    rac_updated_at = v_now,
    updated_at = v_now
  returning total_kudos, rac into v_total, v_rac;

  return jsonb_build_object(
    'ok', true,
    'awarded', true,
    'amount', v_amount,
    'beneficiaryKey', v_key,
    'contentHash', v_hash,
    'totalKudos', v_total,
    'rac', round(v_rac, 4)
  );
end;
$$;

revoke all on function public.kudos_award_soft_verify(text, text, uuid, numeric) from public;
grant execute on function public.kudos_award_soft_verify(text, text, uuid, numeric)
  to service_role;

comment on function public.kudos_award_soft_verify(text, text, uuid, numeric) is
  'Credit capsule author after soft-verify confirm vote; BOINC-style RAC half-life 7d.';
