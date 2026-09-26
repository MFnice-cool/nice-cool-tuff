-- Separate Modpack Index co-occurrence graph from TuffSwarm (launcher) pairs.
-- TuffSwarm stays in public.mod_cooccurrence_pairs.
-- MPI catalog packs (CurseForge + Modrinth via modpackindex.com) → mpi_mod_cooccurrence_pairs.

create table if not exists public.mpi_mod_cooccurrence_pairs (
  mod_a text not null,
  mod_b text not null,
  category_slug text not null default '',
  mc_version text not null default '',
  loader text not null default '',
  count bigint not null default 1,
  updated_at timestamptz not null default now(),
  primary key (mod_a, mod_b, category_slug, mc_version, loader),
  constraint mpi_mod_cooccurrence_ordered check (mod_a < mod_b),
  constraint mpi_mod_cooccurrence_ids_nonempty check (
    char_length(trim(mod_a)) > 0 and char_length(trim(mod_b)) > 0
  ),
  constraint mpi_mod_cooccurrence_count_pos check (count >= 1)
);

create index if not exists mpi_mod_cooccurrence_count_idx
  on public.mpi_mod_cooccurrence_pairs (count desc);

create index if not exists mpi_mod_cooccurrence_cat_count_idx
  on public.mpi_mod_cooccurrence_pairs (category_slug, count desc);

create index if not exists mpi_mod_cooccurrence_loader_mc_count_idx
  on public.mpi_mod_cooccurrence_pairs (loader, mc_version, count desc);

alter table public.mpi_mod_cooccurrence_pairs enable row level security;

drop policy if exists mpi_mod_cooccurrence_pairs_select_anon on public.mpi_mod_cooccurrence_pairs;
create policy mpi_mod_cooccurrence_pairs_select_anon
  on public.mpi_mod_cooccurrence_pairs
  for select
  to anon, authenticated
  using (true);

grant select on public.mpi_mod_cooccurrence_pairs to anon, authenticated;
revoke insert, update, delete on public.mpi_mod_cooccurrence_pairs from anon, authenticated;
grant select, insert, update, delete on public.mpi_mod_cooccurrence_pairs to service_role;

comment on table public.mpi_mod_cooccurrence_pairs is
  'Modpack Index pack co-occurrence (CF+MR via MPI). Separate from TuffSwarm launcher pairs.';

-- Ledger: track category-scoped crawls separately from version×loader crawls.
alter table public.mpi_pack_sync
  add column if not exists category_slug text not null default '';

do $$
declare
  pk_name text;
begin
  select c.conname into pk_name
  from pg_constraint c
  where c.conrelid = 'public.mpi_pack_sync'::regclass
    and c.contype = 'p'
  limit 1;
  if pk_name is not null then
    execute format('alter table public.mpi_pack_sync drop constraint %I', pk_name);
  end if;
  alter table public.mpi_pack_sync
    add primary key (mpi_pack_id, mc_version, loader, category_slug);
exception
  when undefined_table then null;
  when duplicate_table then null;
  when invalid_table_definition then null;
end $$;

-- Weighted bump into MPI-only table.
create or replace function public.bump_mpi_cooccurrence_pairs(pairs jsonb)
returns integer
language plpgsql
security definer
set search_path = public
as $$
declare
  item jsonb;
  a text;
  b text;
  tmp text;
  mc text;
  ld text;
  cat text;
  w bigint;
  n integer := 0;
begin
  if pairs is null or jsonb_typeof(pairs) <> 'array' then
    return 0;
  end if;
  for item in select * from jsonb_array_elements(pairs)
  loop
    a := lower(trim(coalesce(item->>'mod_a', '')));
    b := lower(trim(coalesce(item->>'mod_b', '')));
    mc := trim(coalesce(item->>'mc_version', ''));
    ld := lower(trim(coalesce(item->>'loader', '')));
    cat := lower(trim(coalesce(item->>'category_slug', '')));
    w := greatest(coalesce((item->>'weight')::bigint, 1), 1);
    if a = '' or b = '' or a = b then
      continue;
    end if;
    if a > b then
      tmp := a; a := b; b := tmp;
    end if;
    insert into public.mpi_mod_cooccurrence_pairs as t
      (mod_a, mod_b, category_slug, mc_version, loader, count, updated_at)
    values (a, b, cat, mc, ld, w, now())
    on conflict (mod_a, mod_b, category_slug, mc_version, loader)
    do update set
      count = t.count + excluded.count,
      updated_at = now();
    n := n + 1;
  end loop;
  return n;
end;
$$;

revoke all on function public.bump_mpi_cooccurrence_pairs(jsonb) from public, anon, authenticated;
grant execute on function public.bump_mpi_cooccurrence_pairs(jsonb) to service_role;

create or replace function public.seed_mpi_cooccurrence_pairs(pairs jsonb)
returns integer
language plpgsql
security definer
set search_path = public
as $$
begin
  return public.bump_mpi_cooccurrence_pairs(pairs);
end;
$$;

revoke all on function public.seed_mpi_cooccurrence_pairs(jsonb) from public, anon, authenticated;
grant execute on function public.seed_mpi_cooccurrence_pairs(jsonb) to service_role;

comment on function public.seed_mpi_cooccurrence_pairs(jsonb) is
  'Service-role only: seed/bump Modpack Index co-occurrence pairs (separate from launcher).';

-- Redirect old hub seed path: only write launcher rows (ignore last_source=mpi).
create or replace function public.seed_cooccurrence_pairs(pairs jsonb)
returns integer
language plpgsql
security definer
set search_path = public
as $$
declare
  item jsonb;
  filtered jsonb := '[]'::jsonb;
  src text;
begin
  if pairs is null or jsonb_typeof(pairs) <> 'array' then
    return 0;
  end if;
  for item in select * from jsonb_array_elements(pairs)
  loop
    src := lower(trim(coalesce(item->>'last_source', 'launcher')));
    if src = 'mpi' then
      continue;
    end if;
    filtered := filtered || jsonb_build_array(item);
  end loop;
  return public.bump_mod_cooccurrence_pairs(filtered);
end;
$$;

revoke all on function public.seed_cooccurrence_pairs(jsonb) from public, anon, authenticated;
grant execute on function public.seed_cooccurrence_pairs(jsonb) to service_role;

comment on function public.seed_cooccurrence_pairs(jsonb) is
  'Service-role only: seed TuffSwarm/launcher co-occurrence (skips last_source=mpi).';

-- Partners from MPI graph only.
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
language sql
stable
security invoker
set search_path = public
as $$
  select
    case
      when p.mod_a = lower(trim(p_mod)) then p.mod_b
      else p.mod_a
    end as partner,
    sum(p.count)::bigint as pack_count
  from public.mpi_mod_cooccurrence_pairs p
  where lower(trim(p_mod)) <> ''
    and (p.mod_a = lower(trim(p_mod)) or p.mod_b = lower(trim(p_mod)))
    and (
      p_loader is null
      or trim(p_loader) = ''
      or p.loader = lower(trim(p_loader))
      or p.loader = ''
    )
    and (
      p_mc_version is null
      or trim(p_mc_version) = ''
      or p.mc_version = trim(p_mc_version)
      or p.mc_version = ''
    )
    and (
      p_category_slug is null
      or trim(p_category_slug) = ''
      or p.category_slug = lower(trim(p_category_slug))
    )
  group by 1
  order by pack_count desc, partner asc
  limit greatest(coalesce(p_limit, 20), 1);
$$;

revoke all on function public.partners_for_mod_mpi(text, integer, text, text, text) from public;
grant execute on function public.partners_for_mod_mpi(text, integer, text, text, text)
  to anon, authenticated, service_role;

comment on function public.partners_for_mod_mpi(text, integer, text, text, text) is
  'Top N mods that co-occur with p_mod in Modpack Index packs (separate graph).';

-- One-time move of historical MPI rows out of the launcher table.
insert into public.mpi_mod_cooccurrence_pairs as t
  (mod_a, mod_b, category_slug, mc_version, loader, count, updated_at)
select
  mod_a,
  mod_b,
  '',
  mc_version,
  loader,
  count,
  updated_at
from public.mod_cooccurrence_pairs
where lower(coalesce(last_source, '')) = 'mpi'
on conflict (mod_a, mod_b, category_slug, mc_version, loader)
do update set
  count = greatest(t.count, excluded.count),
  updated_at = greatest(t.updated_at, excluded.updated_at);

delete from public.mod_cooccurrence_pairs
where lower(coalesce(last_source, '')) = 'mpi';
