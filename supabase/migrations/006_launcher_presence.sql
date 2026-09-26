-- Launcher online presence + session duration logs (Supabase-backed).
-- Writes go through security-definer RPCs (anon may call heartbeat/goodbye/stats).
-- Direct INSERT/UPDATE on tables is revoked for anon.

create table if not exists public.launcher_presence (
  device_id text primary key,
  display_name text,
  app_version text,
  last_seen timestamptz not null default now(),
  session_id uuid,
  session_started_at timestamptz,
  updated_at timestamptz not null default now(),
  constraint launcher_presence_device_nonempty check (char_length(trim(device_id)) > 0)
);

create table if not exists public.launcher_sessions (
  id uuid primary key default gen_random_uuid(),
  device_id text not null,
  started_at timestamptz not null default now(),
  ended_at timestamptz,
  duration_seconds integer,
  end_reason text,
  app_version text,
  constraint launcher_sessions_device_nonempty check (char_length(trim(device_id)) > 0),
  constraint launcher_sessions_duration_pos check (
    duration_seconds is null or duration_seconds >= 0
  )
);

create index if not exists launcher_presence_last_seen_idx
  on public.launcher_presence (last_seen desc);

create index if not exists launcher_sessions_started_idx
  on public.launcher_sessions (started_at desc);

create index if not exists launcher_sessions_device_idx
  on public.launcher_sessions (device_id, started_at desc);

-- Online window: heartbeat expected ~30s; consider offline after 90s.
create or replace function public.launcher_online_count()
returns bigint
language sql
stable
security definer
set search_path = public
as $$
  select count(*)::bigint
  from public.launcher_presence
  where last_seen > now() - interval '90 seconds';
$$;

create or replace function public.launcher_heartbeat(
  p_device_id text,
  p_display_name text default null,
  p_app_version text default null
)
returns jsonb
language plpgsql
security definer
set search_path = public
as $$
declare
  v_device text := left(lower(trim(p_device_id)), 128);
  v_name text := nullif(left(trim(coalesce(p_display_name, '')), 64), '');
  v_ver text := nullif(left(trim(coalesce(p_app_version, '')), 32), '');
  v_session uuid;
  v_started timestamptz;
  v_online bigint;
begin
  if v_device = '' then
    raise exception 'device_id required';
  end if;

  select session_id, session_started_at
    into v_session, v_started
  from public.launcher_presence
  where device_id = v_device;

  if v_session is null
     or v_started is null
     or not exists (
       select 1 from public.launcher_sessions s
       where s.id = v_session and s.ended_at is null
     )
  then
    insert into public.launcher_sessions (device_id, app_version)
    values (v_device, v_ver)
    returning id, started_at into v_session, v_started;
  end if;

  insert into public.launcher_presence as t (
    device_id, display_name, app_version, last_seen, session_id, session_started_at, updated_at
  )
  values (v_device, v_name, v_ver, now(), v_session, v_started, now())
  on conflict (device_id) do update set
    display_name = coalesce(excluded.display_name, t.display_name),
    app_version = coalesce(excluded.app_version, t.app_version),
    last_seen = now(),
    session_id = excluded.session_id,
    session_started_at = excluded.session_started_at,
    updated_at = now();

  select public.launcher_online_count() into v_online;

  return jsonb_build_object(
    'ok', true,
    'onlineCount', v_online,
    'sessionId', v_session,
    'sessionStartedAt', v_started
  );
end;
$$;

create or replace function public.launcher_goodbye(
  p_device_id text,
  p_reason text default 'exit'
)
returns jsonb
language plpgsql
security definer
set search_path = public
as $$
declare
  v_device text := left(lower(trim(p_device_id)), 128);
  v_session uuid;
  v_started timestamptz;
  v_reason text := left(trim(coalesce(p_reason, 'exit')), 32);
  v_dur integer;
  v_online bigint;
begin
  if v_device = '' then
    raise exception 'device_id required';
  end if;

  select session_id, session_started_at
    into v_session, v_started
  from public.launcher_presence
  where device_id = v_device;

  if v_session is not null then
    update public.launcher_sessions s
    set
      ended_at = now(),
      duration_seconds = greatest(
        0,
        floor(extract(epoch from (now() - s.started_at)))::integer
      ),
      end_reason = v_reason
    where s.id = v_session
      and s.ended_at is null
    returning duration_seconds into v_dur;
  end if;

  update public.launcher_presence
  set
    session_id = null,
    session_started_at = null,
    last_seen = now() - interval '2 minutes',
    updated_at = now()
  where device_id = v_device;

  select public.launcher_online_count() into v_online;

  return jsonb_build_object(
    'ok', true,
    'onlineCount', v_online,
    'durationSeconds', v_dur,
    'sessionId', v_session
  );
end;
$$;

create or replace function public.launcher_online_stats()
returns jsonb
language sql
stable
security definer
set search_path = public
as $$
  select jsonb_build_object(
    'onlineCount', public.launcher_online_count(),
    'online', coalesce((
      select jsonb_agg(jsonb_build_object(
        'deviceId', device_id,
        'displayName', display_name,
        'appVersion', app_version,
        'lastSeen', last_seen,
        'sessionStartedAt', session_started_at
      ) order by last_seen desc)
      from public.launcher_presence
      where last_seen > now() - interval '90 seconds'
    ), '[]'::jsonb),
    'asOf', now()
  );
$$;

create or replace function public.launcher_recent_sessions(p_limit integer default 50)
returns table (
  id uuid,
  device_id text,
  started_at timestamptz,
  ended_at timestamptz,
  duration_seconds integer,
  end_reason text,
  app_version text
)
language sql
stable
security definer
set search_path = public
as $$
  select
    s.id,
    s.device_id,
    s.started_at,
    s.ended_at,
    s.duration_seconds,
    s.end_reason,
    s.app_version
  from public.launcher_sessions s
  order by s.started_at desc
  limit greatest(coalesce(p_limit, 50), 1);
$$;

alter table public.launcher_presence enable row level security;
alter table public.launcher_sessions enable row level security;

drop policy if exists launcher_presence_deny_direct on public.launcher_presence;
create policy launcher_presence_deny_direct
  on public.launcher_presence
  for all
  to anon, authenticated
  using (false)
  with check (false);

drop policy if exists launcher_sessions_deny_direct on public.launcher_sessions;
create policy launcher_sessions_deny_direct
  on public.launcher_sessions
  for all
  to anon, authenticated
  using (false)
  with check (false);

revoke all on table public.launcher_presence from anon, authenticated;
revoke all on table public.launcher_sessions from anon, authenticated;

revoke all on function public.launcher_online_count() from public;
revoke all on function public.launcher_heartbeat(text, text, text) from public;
revoke all on function public.launcher_goodbye(text, text) from public;
revoke all on function public.launcher_online_stats() from public;
revoke all on function public.launcher_recent_sessions(integer) from public;

grant execute on function public.launcher_online_count() to anon, authenticated, service_role;
grant execute on function public.launcher_heartbeat(text, text, text) to anon, authenticated, service_role;
grant execute on function public.launcher_goodbye(text, text) to anon, authenticated, service_role;
grant execute on function public.launcher_online_stats() to anon, authenticated, service_role;
grant execute on function public.launcher_recent_sessions(integer) to anon, authenticated, service_role;
