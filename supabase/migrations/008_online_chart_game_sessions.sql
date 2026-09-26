-- Online time-series samples + game sessions for admin charts.

create table if not exists public.launcher_online_samples (
  bucket_ts timestamptz primary key,
  online_count integer not null default 0,
  game_online integer not null default 0,
  updated_at timestamptz not null default now(),
  constraint launcher_online_samples_counts_nonneg check (
    online_count >= 0 and game_online >= 0
  )
);

create index if not exists launcher_online_samples_ts_idx
  on public.launcher_online_samples (bucket_ts desc);

alter table public.launcher_online_samples enable row level security;
drop policy if exists launcher_online_samples_deny_direct on public.launcher_online_samples;
create policy launcher_online_samples_deny_direct
  on public.launcher_online_samples
  for all to anon, authenticated
  using (false) with check (false);
revoke all on table public.launcher_online_samples from anon, authenticated;

alter table public.launcher_sessions
  add column if not exists kind text not null default 'launcher',
  add column if not exists project_name text;

alter table public.launcher_sessions
  drop constraint if exists launcher_sessions_kind_check;
alter table public.launcher_sessions
  add constraint launcher_sessions_kind_check
  check (kind in ('launcher', 'game'));

alter table public.launcher_presence
  add column if not exists game_session_id uuid;

create or replace function public.launcher_game_online_count()
returns bigint
language sql
stable
security definer
set search_path = public
as $$
  select count(*)::bigint
  from public.launcher_sessions
  where kind = 'game' and ended_at is null;
$$;

create or replace function public._sample_online_counts()
returns void
language plpgsql
security definer
set search_path = public
as $$
declare
  v_bucket timestamptz := date_trunc('minute', now());
  v_online integer := public.launcher_online_count()::integer;
  v_game integer := public.launcher_game_online_count()::integer;
begin
  insert into public.launcher_online_samples as t
    (bucket_ts, online_count, game_online, updated_at)
  values (v_bucket, v_online, v_game, now())
  on conflict (bucket_ts) do update set
    online_count = greatest(t.online_count, excluded.online_count),
    game_online = greatest(t.game_online, excluded.game_online),
    updated_at = now();
end;
$$;

-- Re-define heartbeat to also write chart samples.
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
  v_game bigint;
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
       where s.id = v_session and s.ended_at is null and s.kind = 'launcher'
     )
  then
    insert into public.launcher_sessions (device_id, app_version, kind)
    values (v_device, v_ver, 'launcher')
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

  perform public._sample_online_counts();
  select public.launcher_online_count() into v_online;
  select public.launcher_game_online_count() into v_game;

  return jsonb_build_object(
    'ok', true,
    'onlineCount', v_online,
    'gameOnline', v_game,
    'sessionId', v_session,
    'sessionStartedAt', v_started
  );
end;
$$;

create or replace function public.launcher_game_session_start(
  p_device_id text,
  p_project_name text default null,
  p_app_version text default null
)
returns jsonb
language plpgsql
security definer
set search_path = public
as $$
declare
  v_device text := left(lower(trim(p_device_id)), 128);
  v_project text := nullif(left(trim(coalesce(p_project_name, '')), 96), '');
  v_ver text := nullif(left(trim(coalesce(p_app_version, '')), 32), '');
  v_session uuid;
  v_started timestamptz;
begin
  if v_device = '' then
    raise exception 'device_id required';
  end if;

  -- Close any previous open game session for this device.
  update public.launcher_sessions
  set
    ended_at = now(),
    duration_seconds = greatest(
      0,
      floor(extract(epoch from (now() - started_at)))::integer
    ),
    end_reason = 'superseded'
  where device_id = v_device
    and kind = 'game'
    and ended_at is null;

  insert into public.launcher_sessions (device_id, app_version, kind, project_name)
  values (v_device, v_ver, 'game', v_project)
  returning id, started_at into v_session, v_started;

  update public.launcher_presence
  set game_session_id = v_session, updated_at = now()
  where device_id = v_device;

  perform public._sample_online_counts();

  return jsonb_build_object(
    'ok', true,
    'sessionId', v_session,
    'startedAt', v_started,
    'gameOnline', public.launcher_game_online_count()
  );
end;
$$;

create or replace function public.launcher_game_session_end(
  p_device_id text,
  p_duration_seconds integer default null,
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
  v_dur integer;
  v_reason text := left(trim(coalesce(p_reason, 'exit')), 32);
begin
  if v_device = '' then
    raise exception 'device_id required';
  end if;

  select game_session_id into v_session
  from public.launcher_presence
  where device_id = v_device;

  if v_session is null then
    select id into v_session
    from public.launcher_sessions
    where device_id = v_device and kind = 'game' and ended_at is null
    order by started_at desc
    limit 1;
  end if;

  if v_session is not null then
    update public.launcher_sessions s
    set
      ended_at = now(),
      duration_seconds = coalesce(
        nullif(p_duration_seconds, -1),
        greatest(0, floor(extract(epoch from (now() - s.started_at)))::integer)
      ),
      end_reason = v_reason
    where s.id = v_session
      and s.ended_at is null
    returning duration_seconds into v_dur;
  end if;

  update public.launcher_presence
  set game_session_id = null, updated_at = now()
  where device_id = v_device;

  perform public._sample_online_counts();

  return jsonb_build_object(
    'ok', true,
    'sessionId', v_session,
    'durationSeconds', v_dur,
    'gameOnline', public.launcher_game_online_count()
  );
end;
$$;

create or replace function public.launcher_online_series(p_hours integer default 24)
returns jsonb
language sql
stable
security definer
set search_path = public
as $$
  select jsonb_build_object(
    'ok', true,
    'hours', greatest(coalesce(p_hours, 24), 1),
    'points', coalesce((
      select jsonb_agg(jsonb_build_object(
        't', bucket_ts,
        'online', online_count,
        'game', game_online
      ) order by bucket_ts)
      from public.launcher_online_samples
      where bucket_ts >= now() - make_interval(hours => greatest(coalesce(p_hours, 24), 1))
    ), '[]'::jsonb),
    'onlineNow', public.launcher_online_count(),
    'gameOnlineNow', public.launcher_game_online_count()
  );
$$;

drop function if exists public.launcher_recent_sessions(integer);
create function public.launcher_recent_sessions(p_limit integer default 50)
returns table (
  id uuid,
  device_id text,
  started_at timestamptz,
  ended_at timestamptz,
  duration_seconds integer,
  end_reason text,
  app_version text,
  kind text,
  project_name text
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
    s.app_version,
    s.kind,
    s.project_name
  from public.launcher_sessions s
  order by s.started_at desc
  limit greatest(coalesce(p_limit, 50), 1);
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
    'gameOnline', public.launcher_game_online_count(),
    'online', coalesce((
      select jsonb_agg(jsonb_build_object(
        'deviceId', device_id,
        'displayName', display_name,
        'appVersion', app_version,
        'lastSeen', last_seen,
        'sessionStartedAt', session_started_at,
        'gameSessionId', game_session_id
      ) order by last_seen desc)
      from public.launcher_presence
      where last_seen > now() - interval '90 seconds'
    ), '[]'::jsonb),
    'asOf', now()
  );
$$;

revoke all on function public.launcher_game_online_count() from public;
revoke all on function public._sample_online_counts() from public;
revoke all on function public.launcher_game_session_start(text, text, text) from public;
revoke all on function public.launcher_game_session_end(text, integer, text) from public;
revoke all on function public.launcher_online_series(integer) from public;

grant execute on function public.launcher_game_online_count() to anon, authenticated, service_role;
grant execute on function public.launcher_game_session_start(text, text, text) to anon, authenticated, service_role;
grant execute on function public.launcher_game_session_end(text, integer, text) to anon, authenticated, service_role;
grant execute on function public.launcher_online_series(integer) to anon, authenticated, service_role;
-- heartbeat / online_stats / recent_sessions already granted; recreate keeps grants on replace for same signature.
grant execute on function public.launcher_heartbeat(text, text, text) to anon, authenticated, service_role;
grant execute on function public.launcher_online_stats() to anon, authenticated, service_role;
grant execute on function public.launcher_recent_sessions(integer) to anon, authenticated, service_role;
