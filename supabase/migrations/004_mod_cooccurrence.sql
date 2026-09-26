-- Mod co-occurrence pairs for Create Mode AI (which mods appear together in packs).
-- Writes MUST go through the report-cooccurrence Edge Function (service role).
-- Anon / authenticated clients may only SELECT.

create table if not exists public.mod_cooccurrence_pairs (
  mod_a text not null,
  mod_b text not null,
  mc_version text not null default '',
  loader text not null default '',
  count bigint not null default 1,
  last_source text,
  updated_at timestamptz not null default now(),
  primary key (mod_a, mod_b, mc_version, loader),
  constraint mod_cooccurrence_ordered check (mod_a < mod_b),
  constraint mod_cooccurrence_ids_nonempty check (
    char_length(trim(mod_a)) > 0 and char_length(trim(mod_b)) > 0
  ),
  constraint mod_cooccurrence_count_pos check (count >= 1)
);

create index if not exists mod_cooccurrence_loader_mc_count_idx
  on public.mod_cooccurrence_pairs (loader, mc_version, count desc);

create index if not exists mod_cooccurrence_count_idx
  on public.mod_cooccurrence_pairs (count desc);

-- Rate-limit windows for report-cooccurrence (service role only).
create table if not exists public.mod_cooccurrence_rate (
  client_key text primary key,
  window_start timestamptz not null default now(),
  report_count integer not null default 0
);

-- Atomic bump used by Edge Function (service role / security definer).
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
    if a = '' or b = '' or a = b then
      continue;
    end if;
    if a > b then
      tmp := a; a := b; b := tmp;
    end if;
    insert into public.mod_cooccurrence_pairs as t
      (mod_a, mod_b, mc_version, loader, count, last_source, updated_at)
    values (a, b, mc, ld, 1, nullif(src, ''), now())
    on conflict (mod_a, mod_b, mc_version, loader)
    do update set
      count = t.count + 1,
      last_source = excluded.last_source,
      updated_at = now();
    n := n + 1;
  end loop;
  return n;
end;
$$;

revoke all on function public.bump_mod_cooccurrence_pairs(jsonb) from public, anon, authenticated;
grant execute on function public.bump_mod_cooccurrence_pairs(jsonb) to service_role;

alter table public.mod_cooccurrence_pairs enable row level security;
alter table public.mod_cooccurrence_rate enable row level security;

drop policy if exists mod_cooccurrence_pairs_select_anon on public.mod_cooccurrence_pairs;
create policy mod_cooccurrence_pairs_select_anon
  on public.mod_cooccurrence_pairs
  for select
  to anon, authenticated
  using (true);

drop policy if exists mod_cooccurrence_rate_deny_all on public.mod_cooccurrence_rate;
create policy mod_cooccurrence_rate_deny_all
  on public.mod_cooccurrence_rate
  for all
  to anon, authenticated
  using (false)
  with check (false);

grant select on public.mod_cooccurrence_pairs to anon, authenticated;
revoke insert, update, delete on public.mod_cooccurrence_pairs from anon, authenticated;
revoke all on public.mod_cooccurrence_rate from anon, authenticated;
