-- Creation Marketplace Accept → Kudos (off-chain).
-- Customer explicit Accept awards the worker (device signer_public_key).
-- content_hash for creation rows uses sentinel `creation:{job_id}` (no capsule FK).

-- Allow ledger rows that are not capsule hashes (creation accept).
alter table public.kudos_ledger
  drop constraint if exists kudos_ledger_content_hash_fkey;

alter table public.kudos_ledger
  drop constraint if exists kudos_ledger_reason_check;

alter table public.kudos_ledger
  add constraint kudos_ledger_reason_check
  check (reason in ('soft_verify_confirm', 'creation_accept'));

comment on column public.kudos_ledger.content_hash is
  'Capsule content_hash for soft_verify_confirm; or creation:{jobId} for creation_accept (no FK).';

-- Award after customer Accept of a CreationResult. Idempotent per (hash, voter, reason)
-- where hash = creation:{job_id}.
create or replace function public.kudos_award_creation_accept(
  p_beneficiary_key text,
  p_job_id text,
  p_voter_user_id uuid,
  p_amount numeric default 50
)
returns jsonb
language plpgsql
security definer
set search_path = public, pg_temp
as $$
declare
  v_key text := trim(p_beneficiary_key);
  v_job text := trim(p_job_id);
  v_hash text;
  v_amount numeric := greatest(0.01, coalesce(p_amount, 50));
  v_now timestamptz := now();
  v_total numeric;
  v_rac numeric;
  v_prev_rac numeric;
  v_prev_at timestamptz;
begin
  if v_key = '' or v_job = '' or p_voter_user_id is null then
    return jsonb_build_object('ok', false, 'awarded', false, 'reason', 'missing_args');
  end if;

  v_hash := 'creation:' || v_job;

  insert into public.kudos_ledger (beneficiary_key, amount, reason, content_hash, voter_user_id)
  values (v_key, v_amount, 'creation_accept', v_hash, p_voter_user_id)
  on conflict (content_hash, voter_user_id, reason) do nothing;

  if not found then
    select total_kudos, rac into v_total, v_rac
    from public.kudos_balances
    where beneficiary_key = v_key;
    return jsonb_build_object(
      'ok', true,
      'awarded', false,
      'reason', 'already_awarded',
      'beneficiaryKey', v_key,
      'jobId', v_job,
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
    'jobId', v_job,
    'contentHash', v_hash,
    'totalKudos', v_total,
    'rac', round(v_rac, 4)
  );
end;
$$;

revoke all on function public.kudos_award_creation_accept(text, text, uuid, numeric) from public;
grant execute on function public.kudos_award_creation_accept(text, text, uuid, numeric)
  to service_role;

comment on function public.kudos_award_creation_accept(text, text, uuid, numeric) is
  'Credit Creation worker after customer Accept; BOINC-style RAC half-life 7d. Not for worker reply.';
