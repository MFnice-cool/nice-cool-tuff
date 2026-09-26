-- Custom Optimize catalog: mods per loader × Minecraft version.
-- Seeded from Fabulously Optimized (Fabric); Forge/NeoForge filled later by custom packs.
-- Launcher resolves the newest Modrinth version at install time — we only store slugs.

create table if not exists public.optimize_mod_matrix (
  loader text not null,
  mc_version text not null,
  modrinth_slug text not null,
  sort_order integer not null default 0,
  name text,
  source text not null default 'fabulously-optimized',
  source_version_id text,
  updated_at timestamptz not null default now(),
  primary key (loader, mc_version, modrinth_slug),
  constraint optimize_mod_matrix_loader_check
    check (loader in ('fabric', 'quilt', 'forge', 'neoforge')),
  constraint optimize_mod_matrix_slug_nonempty
    check (char_length(trim(modrinth_slug)) > 0),
  constraint optimize_mod_matrix_mc_nonempty
    check (char_length(trim(mc_version)) > 0)
);

create index if not exists optimize_mod_matrix_lookup_idx
  on public.optimize_mod_matrix (loader, mc_version, sort_order);

alter table public.optimize_mod_matrix enable row level security;

drop policy if exists optimize_mod_matrix_select_anon on public.optimize_mod_matrix;
create policy optimize_mod_matrix_select_anon
  on public.optimize_mod_matrix
  for select
  to anon, authenticated
  using (true);

grant select on public.optimize_mod_matrix to anon, authenticated;
revoke insert, update, delete on public.optimize_mod_matrix from anon, authenticated;
grant select, insert, update, delete on public.optimize_mod_matrix to service_role;

comment on table public.optimize_mod_matrix is
  'Custom Optimize mod slugs per loader×MC. Fabric rows come from Fabulously Optimized; other loaders from TuffBox packs.';

-- Replace all rows for one loader×MC (service_role seed scripts).
create or replace function public.replace_optimize_mods_for(
  p_loader text,
  p_mc_version text,
  p_rows jsonb
)
returns integer
language plpgsql
security definer
set search_path = public
as $$
declare
  n integer := 0;
  loader_norm text := lower(trim(p_loader));
  mc_norm text := trim(p_mc_version);
begin
  if loader_norm is null or loader_norm = '' or mc_norm is null or mc_norm = '' then
    raise exception 'loader and mc_version required';
  end if;
  if jsonb_typeof(p_rows) is distinct from 'array' then
    raise exception 'p_rows must be a JSON array';
  end if;

  delete from public.optimize_mod_matrix
  where loader = loader_norm and mc_version = mc_norm;

  insert into public.optimize_mod_matrix (
    loader, mc_version, modrinth_slug, sort_order, name, source, source_version_id, updated_at
  )
  select
    loader_norm,
    mc_norm,
    lower(trim(r.modrinth_slug)),
    coalesce(r.sort_order, ord.ord - 1),
    nullif(trim(r.name), ''),
    coalesce(nullif(trim(r.source), ''), 'fabulously-optimized'),
    nullif(trim(r.source_version_id), ''),
    now()
  from jsonb_array_elements(p_rows) with ordinality as ord(elem, ord)
  cross join lateral (
    select
      elem->>'modrinth_slug' as modrinth_slug,
      (elem->>'sort_order')::integer as sort_order,
      elem->>'name' as name,
      elem->>'source' as source,
      elem->>'source_version_id' as source_version_id
  ) r
  where coalesce(trim(r.modrinth_slug), '') <> ''
  on conflict (loader, mc_version, modrinth_slug) do update set
    sort_order = excluded.sort_order,
    name = excluded.name,
    source = excluded.source,
    source_version_id = excluded.source_version_id,
    updated_at = now();

  get diagnostics n = row_count;
  return n;
end;
$$;

revoke all on function public.replace_optimize_mods_for(text, text, jsonb) from public;
revoke all on function public.replace_optimize_mods_for(text, text, jsonb) from anon, authenticated;
grant execute on function public.replace_optimize_mods_for(text, text, jsonb) to service_role;

-- Public read path for the launcher.
create or replace function public.optimize_mods_for(
  p_loader text,
  p_mc_version text
)
returns table (
  modrinth_slug text,
  sort_order integer,
  name text,
  source text
)
language plpgsql
stable
security invoker
set search_path = public
as $$
declare
  loader_norm text := lower(trim(p_loader));
  mc_norm text := trim(p_mc_version);
  effective_loader text;
begin
  if loader_norm is null or loader_norm = '' or mc_norm is null or mc_norm = '' then
    return;
  end if;

  -- Quilt reuses Fabric FO matrix when no quilt-specific rows exist.
  effective_loader := loader_norm;
  if loader_norm = 'quilt'
     and not exists (
       select 1 from public.optimize_mod_matrix m
       where m.loader = 'quilt' and m.mc_version = mc_norm
     )
  then
    effective_loader := 'fabric';
  end if;

  if exists (
    select 1 from public.optimize_mod_matrix m
    where m.loader = effective_loader and m.mc_version = mc_norm
  ) then
    return query
    select m.modrinth_slug, m.sort_order, m.name, m.source
    from public.optimize_mod_matrix m
    where m.loader = effective_loader and m.mc_version = mc_norm
    order by m.sort_order asc, m.modrinth_slug asc;
    return;
  end if;

  -- Fallback: same loader's "default" profile row if present.
  return query
  select m.modrinth_slug, m.sort_order, m.name, m.source
  from public.optimize_mod_matrix m
  where m.loader = effective_loader and m.mc_version = 'default'
  order by m.sort_order asc, m.modrinth_slug asc;
end;
$$;

revoke all on function public.optimize_mods_for(text, text) from public;
grant execute on function public.optimize_mods_for(text, text) to anon, authenticated, service_role;

comment on function public.optimize_mods_for(text, text) is
  'Custom Optimize mod list for loader+MC. Quilt falls back to Fabric FO rows.';
