-- MPI pack sync ledger + weighted co-occurrence bumps for hub analytics.

create table if not exists public.mpi_pack_sync (
  mpi_pack_id bigint not null,
  mc_version text not null default '',
  loader text not null default '',
  mod_count integer not null default 0,
  synced_at timestamptz not null default now(),
  primary key (mpi_pack_id, mc_version, loader)
);

alter table public.mpi_pack_sync enable row level security;

drop policy if exists mpi_pack_sync_deny_anon on public.mpi_pack_sync;
create policy mpi_pack_sync_deny_anon
  on public.mpi_pack_sync
  for all
  to anon, authenticated
  using (false)
  with check (false);

revoke all on public.mpi_pack_sync from anon, authenticated;
grant select, insert, update, delete on public.mpi_pack_sync to service_role;

-- Weighted bump: optional weight (default 1) for popular pack observations.
create or replace function public.bump_mod_cooccurrence_pairs(pairs jsonb)
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
  src text;
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
    src := left(trim(coalesce(item->>'last_source', 'launcher')), 48);
    w := greatest(coalesce((item->>'weight')::bigint, 1), 1);
    if a = '' or b = '' or a = b then
      continue;
    end if;
    if a > b then
      tmp := a; a := b; b := tmp;
    end if;
    insert into public.mod_cooccurrence_pairs as t
      (mod_a, mod_b, mc_version, loader, count, last_source, updated_at)
    values (a, b, mc, ld, w, nullif(src, ''), now())
    on conflict (mod_a, mod_b, mc_version, loader)
    do update set
      count = t.count + excluded.count,
      last_source = excluded.last_source,
      updated_at = now();
    n := n + 1;
  end loop;
  return n;
end;
$$;

revoke all on function public.bump_mod_cooccurrence_pairs(jsonb) from public, anon, authenticated;
grant execute on function public.bump_mod_cooccurrence_pairs(jsonb) to service_role;

-- Hub seed path: bump pairs without client rate-limit table.
create or replace function public.seed_cooccurrence_pairs(pairs jsonb)
returns integer
language plpgsql
security definer
set search_path = public
as $$
begin
  return public.bump_mod_cooccurrence_pairs(pairs);
end;
$$;

revoke all on function public.seed_cooccurrence_pairs(jsonb) from public, anon, authenticated;
grant execute on function public.seed_cooccurrence_pairs(jsonb) to service_role;

comment on function public.seed_cooccurrence_pairs(jsonb) is
  'Service-role only: seed/bump co-occurrence pairs (hub MPI analytics).';

comment on table public.mpi_pack_sync is
  'Idempotency ledger for Modpack Index pack→pair sync jobs.';
