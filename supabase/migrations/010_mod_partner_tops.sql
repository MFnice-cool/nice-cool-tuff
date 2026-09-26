-- Precomputed top-20 co-occurrence partners (JSONB hot path).
-- Pair tables remain source of truth; this cache is refreshed by hub / service_role.
-- Optional: schedule via pg_cron if available, e.g.
--   select cron.schedule('refresh-mod-partner-tops', '0 */6 * * *',
--     $$select public.refresh_mod_partner_tops()$$);

create table if not exists public.mod_partner_tops (
  mod_id text not null,
  graph text not null,
  loader text not null default '',
  mc_version text not null default '',
  partners jsonb not null default '[]'::jsonb,
  updated_at timestamptz not null default now(),
  primary key (mod_id, graph, loader, mc_version),
  constraint mod_partner_tops_graph_check check (graph in ('launcher', 'mpi')),
  constraint mod_partner_tops_mod_nonempty check (char_length(trim(mod_id)) > 0),
  constraint mod_partner_tops_partners_array check (jsonb_typeof(partners) = 'array')
);

create index if not exists mod_partner_tops_graph_updated_idx
  on public.mod_partner_tops (graph, updated_at desc);

alter table public.mod_partner_tops enable row level security;

drop policy if exists mod_partner_tops_select_anon on public.mod_partner_tops;
create policy mod_partner_tops_select_anon
  on public.mod_partner_tops
  for select
  to anon, authenticated
  using (true);

grant select on public.mod_partner_tops to anon, authenticated;
revoke insert, update, delete on public.mod_partner_tops from anon, authenticated;
grant select, insert, update, delete on public.mod_partner_tops to service_role;

comment on table public.mod_partner_tops is
  'Hot-path top-20 partners JSONB per mod (launcher / mpi). Refreshed by refresh_mod_partner_tops.';

-- Rebuild entire cache from pair tables (global '' + scoped loader×mc).
create or replace function public.refresh_mod_partner_tops()
returns integer
language plpgsql
security definer
set search_path = public
as $$
declare
  n integer := 0;
begin
  delete from public.mod_partner_tops;

  -- Launcher: global aggregate (loader='', mc_version='')
  insert into public.mod_partner_tops (mod_id, graph, loader, mc_version, partners, updated_at)
  select
    r.mod_id,
    'launcher',
    '',
    '',
    jsonb_agg(
      jsonb_build_object('partner', r.partner, 'pack_count', r.pack_count)
      order by r.pack_count desc, r.partner asc
    ),
    now()
  from (
    select
      e.mod_id,
      e.partner,
      e.pack_count,
      row_number() over (
        partition by e.mod_id
        order by e.pack_count desc, e.partner asc
      ) as rn
    from (
      select
        x.mod_id,
        x.partner,
        sum(x.cnt)::bigint as pack_count
      from (
        select mod_a as mod_id, mod_b as partner, count as cnt
        from public.mod_cooccurrence_pairs
        union all
        select mod_b, mod_a, count
        from public.mod_cooccurrence_pairs
      ) x
      group by x.mod_id, x.partner
    ) e
  ) r
  where r.rn <= 20
  group by r.mod_id;

  get diagnostics n = row_count;

  -- Launcher: scoped (loader, mc_version) where either is non-empty
  insert into public.mod_partner_tops (mod_id, graph, loader, mc_version, partners, updated_at)
  select
    r.mod_id,
    'launcher',
    r.loader,
    r.mc_version,
    jsonb_agg(
      jsonb_build_object('partner', r.partner, 'pack_count', r.pack_count)
      order by r.pack_count desc, r.partner asc
    ),
    now()
  from (
    select
      e.mod_id,
      e.loader,
      e.mc_version,
      e.partner,
      e.pack_count,
      row_number() over (
        partition by e.mod_id, e.loader, e.mc_version
        order by e.pack_count desc, e.partner asc
      ) as rn
    from (
      select
        x.mod_id,
        x.loader,
        x.mc_version,
        x.partner,
        sum(x.cnt)::bigint as pack_count
      from (
        select mod_a as mod_id, mod_b as partner, loader, mc_version, count as cnt
        from public.mod_cooccurrence_pairs
        where loader <> '' or mc_version <> ''
        union all
        select mod_b, mod_a, loader, mc_version, count
        from public.mod_cooccurrence_pairs
        where loader <> '' or mc_version <> ''
      ) x
      group by x.mod_id, x.loader, x.mc_version, x.partner
    ) e
  ) r
  where r.rn <= 20
  group by r.mod_id, r.loader, r.mc_version;

  -- MPI: global
  insert into public.mod_partner_tops (mod_id, graph, loader, mc_version, partners, updated_at)
  select
    r.mod_id,
    'mpi',
    '',
    '',
    jsonb_agg(
      jsonb_build_object('partner', r.partner, 'pack_count', r.pack_count)
      order by r.pack_count desc, r.partner asc
    ),
    now()
  from (
    select
      e.mod_id,
      e.partner,
      e.pack_count,
      row_number() over (
        partition by e.mod_id
        order by e.pack_count desc, e.partner asc
      ) as rn
    from (
      select
        x.mod_id,
        x.partner,
        sum(x.cnt)::bigint as pack_count
      from (
        select mod_a as mod_id, mod_b as partner, count as cnt
        from public.mpi_mod_cooccurrence_pairs
        union all
        select mod_b, mod_a, count
        from public.mpi_mod_cooccurrence_pairs
      ) x
      group by x.mod_id, x.partner
    ) e
  ) r
  where r.rn <= 20
  group by r.mod_id;

  -- MPI: scoped
  insert into public.mod_partner_tops (mod_id, graph, loader, mc_version, partners, updated_at)
  select
    r.mod_id,
    'mpi',
    r.loader,
    r.mc_version,
    jsonb_agg(
      jsonb_build_object('partner', r.partner, 'pack_count', r.pack_count)
      order by r.pack_count desc, r.partner asc
    ),
    now()
  from (
    select
      e.mod_id,
      e.loader,
      e.mc_version,
      e.partner,
      e.pack_count,
      row_number() over (
        partition by e.mod_id, e.loader, e.mc_version
        order by e.pack_count desc, e.partner asc
      ) as rn
    from (
      select
        x.mod_id,
        x.loader,
        x.mc_version,
        x.partner,
        sum(x.cnt)::bigint as pack_count
      from (
        select mod_a as mod_id, mod_b as partner, loader, mc_version, count as cnt
        from public.mpi_mod_cooccurrence_pairs
        where loader <> '' or mc_version <> ''
        union all
        select mod_b, mod_a, loader, mc_version, count
        from public.mpi_mod_cooccurrence_pairs
        where loader <> '' or mc_version <> ''
      ) x
      group by x.mod_id, x.loader, x.mc_version, x.partner
    ) e
  ) r
  where r.rn <= 20
  group by r.mod_id, r.loader, r.mc_version;

  select count(*)::integer into n from public.mod_partner_tops;
  return n;
end;
$$;

revoke all on function public.refresh_mod_partner_tops() from public, anon, authenticated;
grant execute on function public.refresh_mod_partner_tops() to service_role;

comment on function public.refresh_mod_partner_tops() is
  'Service-role only: rebuild mod_partner_tops JSONB cache from launcher + MPI pair tables.';

-- partners_for_mod: cache hit (exact → global) then live fallback.
create or replace function public.partners_for_mod(
  p_mod text,
  p_limit integer default 20,
  p_loader text default null,
  p_mc_version text default null
)
returns table (
  partner text,
  pack_count bigint
)
language plpgsql
stable
security invoker
set search_path = public
as $$
declare
  mid text := lower(trim(coalesce(p_mod, '')));
  lim integer := greatest(coalesce(p_limit, 20), 1);
  ld text := lower(trim(coalesce(p_loader, '')));
  mc text := trim(coalesce(p_mc_version, ''));
  cached jsonb;
begin
  if mid = '' then
    return;
  end if;

  select t.partners into cached
  from public.mod_partner_tops t
  where t.mod_id = mid
    and t.graph = 'launcher'
    and t.loader = ld
    and t.mc_version = mc
  limit 1;

  if cached is null and (ld <> '' or mc <> '') then
    select t.partners into cached
    from public.mod_partner_tops t
    where t.mod_id = mid
      and t.graph = 'launcher'
      and t.loader = ''
      and t.mc_version = ''
    limit 1;
  end if;

  if cached is not null and jsonb_typeof(cached) = 'array' and jsonb_array_length(cached) > 0 then
    return query
    select
      trim(elem->>'partner') as partner,
      greatest(coalesce((elem->>'pack_count')::bigint, 1), 1) as pack_count
    from jsonb_array_elements(cached) as elem
    where trim(coalesce(elem->>'partner', '')) <> ''
    limit lim;
    return;
  end if;

  -- Live fallback until first refresh / cache miss.
  return query
  select
    case
      when p.mod_a = mid then p.mod_b
      else p.mod_a
    end as partner,
    sum(p.count)::bigint as pack_count
  from public.mod_cooccurrence_pairs p
  where (p.mod_a = mid or p.mod_b = mid)
    and (ld = '' or p.loader = ld)
    and (mc = '' or p.mc_version = mc)
  group by 1
  order by pack_count desc, partner asc
  limit lim;
end;
$$;

revoke all on function public.partners_for_mod(text, integer, text, text) from public;
grant execute on function public.partners_for_mod(text, integer, text, text)
  to anon, authenticated, service_role;

comment on function public.partners_for_mod(text, integer, text, text) is
  'Top N companions from mod_partner_tops (launcher); live fallback on cache miss.';

-- partners_for_mod_mpi: cache when category unset; category filter stays live-only.
create or replace function public.partners_for_mod_mpi(
  p_mod text,
  p_limit integer default 20,
  p_loader text default null,
  p_mc_version text default null,
  p_category_slug text default null
)
returns table (
  partner text,
  pack_count bigint
)
language plpgsql
stable
security invoker
set search_path = public
as $$
declare
  mid text := lower(trim(coalesce(p_mod, '')));
  lim integer := greatest(coalesce(p_limit, 20), 1);
  ld text := lower(trim(coalesce(p_loader, '')));
  mc text := trim(coalesce(p_mc_version, ''));
  cat text := lower(trim(coalesce(p_category_slug, '')));
  cached jsonb;
begin
  if mid = '' then
    return;
  end if;

  -- Category-scoped queries skip cache (v1: no category in PK).
  if cat = '' then
    select t.partners into cached
    from public.mod_partner_tops t
    where t.mod_id = mid
      and t.graph = 'mpi'
      and t.loader = ld
      and t.mc_version = mc
    limit 1;

    if cached is null and (ld <> '' or mc <> '') then
      select t.partners into cached
      from public.mod_partner_tops t
      where t.mod_id = mid
        and t.graph = 'mpi'
        and t.loader = ''
        and t.mc_version = ''
      limit 1;
    end if;

    if cached is not null and jsonb_typeof(cached) = 'array' and jsonb_array_length(cached) > 0 then
      return query
      select
        trim(elem->>'partner') as partner,
        greatest(coalesce((elem->>'pack_count')::bigint, 1), 1) as pack_count
      from jsonb_array_elements(cached) as elem
      where trim(coalesce(elem->>'partner', '')) <> ''
      limit lim;
      return;
    end if;
  end if;

  return query
  select
    case
      when p.mod_a = mid then p.mod_b
      else p.mod_a
    end as partner,
    sum(p.count)::bigint as pack_count
  from public.mpi_mod_cooccurrence_pairs p
  where (p.mod_a = mid or p.mod_b = mid)
    and (ld = '' or p.loader = ld or p.loader = '')
    and (mc = '' or p.mc_version = mc or p.mc_version = '')
    and (cat = '' or p.category_slug = cat)
  group by 1
  order by pack_count desc, partner asc
  limit lim;
end;
$$;

revoke all on function public.partners_for_mod_mpi(text, integer, text, text, text) from public;
grant execute on function public.partners_for_mod_mpi(text, integer, text, text, text)
  to anon, authenticated, service_role;

comment on function public.partners_for_mod_mpi(text, integer, text, text, text) is
  'Top N MPI companions from mod_partner_tops; live fallback; category filter always live.';
