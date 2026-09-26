-- Admin moderation for community crash-fix capsules.
-- Player votes only accumulate counts; Accept/Reject is decided in admin panel.

create table if not exists public.admin_config (
  key text primary key,
  value text not null,
  updated_at timestamptz not null default now()
);

alter table public.admin_config enable row level security;
drop policy if exists admin_config_deny_all on public.admin_config;
create policy admin_config_deny_all
  on public.admin_config
  for all
  to anon, authenticated
  using (false)
  with check (false);
revoke all on table public.admin_config from anon, authenticated;

-- Default secret — MUST be changed after deploy (see 015_security_hardening.sql).
-- Do not ship a known default in production.
insert into public.admin_config (key, value)
values ('moderation_secret', '')
on conflict (key) do nothing;

alter table public.experience_capsules
  add column if not exists moderated_at timestamptz,
  add column if not exists moderation_note text;

create or replace function public._admin_secret_ok(p_secret text)
returns boolean
language sql
stable
security definer
set search_path = public
as $$
  select coalesce(
    (
      select trim(value) = trim(p_secret)
      from public.admin_config
      where key = 'moderation_secret'
      limit 1
    ),
    false
  )
  and char_length(trim(coalesce(p_secret, ''))) >= 8;
$$;

revoke all on function public._admin_secret_ok(text) from public;

-- Moderation queue: open first (by vote heat), then saved/rejected.
create or replace function public.list_crash_capsules_moderation(
  p_admin_secret text,
  p_status text default 'open',
  p_limit integer default 50
)
returns jsonb
language plpgsql
stable
security definer
set search_path = public
as $$
declare
  v_status text := lower(trim(coalesce(p_status, 'open')));
  v_limit integer := greatest(coalesce(p_limit, 50), 1);
  v_rows jsonb;
begin
  if not public._admin_secret_ok(p_admin_secret) then
    raise exception 'unauthorized';
  end if;
  if v_status not in ('open', 'saved', 'rejected', 'all') then
    raise exception 'status must be open|saved|rejected|all';
  end if;

  select coalesce(jsonb_agg(row_to_json(t)::jsonb order by t.sort_heat desc, t.created_at desc), '[]'::jsonb)
  into v_rows
  from (
    select
      c.content_hash,
      c.id,
      c.fingerprint_key,
      c.mc_major,
      c.loader,
      c.solution,
      c.actions,
      c.status,
      c.trust_score,
      c.confirm_count,
      c.reject_count,
      c.success_count,
      c.fail_count,
      c.created_at,
      c.updated_at,
      c.moderated_at,
      c.moderation_note,
      c.payload,
      (c.confirm_count + c.reject_count) as sort_heat
    from public.experience_capsules c
    where
      case v_status
        when 'all' then true
        else c.status = v_status
      end
    order by (c.confirm_count + c.reject_count) desc, c.created_at desc
    limit v_limit
  ) t;

  return jsonb_build_object(
    'ok', true,
    'status', v_status,
    'capsules', v_rows
  );
end;
$$;

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

  v_trust := v_row.confirm_count::float / (v_row.confirm_count + v_row.reject_count + 1);

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

revoke all on function public.list_crash_capsules_moderation(text, text, integer) from public;
revoke all on function public.moderate_crash_capsule(text, text, text, text) from public;
grant execute on function public.list_crash_capsules_moderation(text, text, integer)
  to anon, authenticated, service_role;
grant execute on function public.moderate_crash_capsule(text, text, text, text)
  to anon, authenticated, service_role;
