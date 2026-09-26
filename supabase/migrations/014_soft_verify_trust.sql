-- Soft-verify trust: rejects/rollbacks weigh 2× confirms (NotebookLM / PoUW soft ranking).
-- Lookup order unchanged: trust_score.desc, success_count.desc.

create or replace function public.capsule_trust_score(p_confirm integer, p_reject integer)
returns double precision
language sql
immutable
as $$
  select greatest(0.0, coalesce(p_confirm, 0)::float)
       / (coalesce(p_confirm, 0)::float + 2.0 * coalesce(p_reject, 0)::float + 1.0);
$$;

comment on function public.capsule_trust_score(integer, integer) is
  'Soft-verify rank: confirm / (confirm + 2*reject + 1). Rejects cost twice as much as keeps.';

revoke all on function public.capsule_trust_score(integer, integer) from public;
grant execute on function public.capsule_trust_score(integer, integer)
  to anon, authenticated, service_role;

-- Backfill existing rows with the weighted formula (status unchanged).
update public.experience_capsules
set trust_score = public.capsule_trust_score(confirm_count, reject_count),
    success_score = case
      when status = 'rejected' then 0
      when status = 'saved' then greatest(0.35, least(1.0, public.capsule_trust_score(confirm_count, reject_count)))
      else least(0.2, public.capsule_trust_score(confirm_count, reject_count))
    end,
    updated_at = now()
where true;

-- Keep moderation RPC in sync with weighted trust.
create or replace function public.moderate_crash_capsule(
  p_admin_secret text,
  p_content_hash text,
  p_decision text,
  p_note text default null
)
returns jsonb
language plpgsql
security definer
set search_path = public
as $$
declare
  v_hash text := trim(p_content_hash);
  v_decision text := lower(trim(p_decision));
  v_note text := nullif(left(trim(coalesce(p_note, '')), 280), '');
  v_row public.experience_capsules%rowtype;
  v_trust double precision;
begin
  if not public._admin_secret_ok(p_admin_secret) then
    raise exception 'unauthorized';
  end if;
  if v_hash = '' then
    raise exception 'content_hash required';
  end if;
  if v_decision not in ('saved', 'rejected', 'open') then
    raise exception 'decision must be saved|rejected|open';
  end if;

  select * into v_row
  from public.experience_capsules
  where content_hash = v_hash;
  if not found then
    raise exception 'capsule not found';
  end if;

  v_trust := public.capsule_trust_score(v_row.confirm_count, v_row.reject_count);

  update public.experience_capsules
  set
    status = v_decision,
    trust_score = v_trust,
    success_score = case
      when v_decision = 'rejected' then 0
      when v_decision = 'saved' then greatest(0.35, least(1.0, v_trust))
      else least(0.2, v_trust)
    end,
    moderated_at = now(),
    moderation_note = v_note,
    updated_at = now()
  where content_hash = v_hash
  returning * into v_row;

  return jsonb_build_object(
    'ok', true,
    'contentHash', v_row.content_hash,
    'status', v_row.status,
    'confirmCount', v_row.confirm_count,
    'rejectCount', v_row.reject_count,
    'trustScore', v_row.trust_score,
    'moderatedAt', v_row.moderated_at
  );
end;
$$;
