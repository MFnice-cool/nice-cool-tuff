-- Top companion mods for a given mod id (frequency across reported packs).
-- Use from SQL editor / API:
--   select * from partners_for_mod('sodium', 20);
-- Optional filters: loader, mc_version
--   select * from partners_for_mod('sodium', 20, 'fabric', '1.20.1');

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
  from public.mod_cooccurrence_pairs p
  where lower(trim(p_mod)) <> ''
    and (p.mod_a = lower(trim(p_mod)) or p.mod_b = lower(trim(p_mod)))
    and (
      p_loader is null
      or trim(p_loader) = ''
      or p.loader = lower(trim(p_loader))
    )
    and (
      p_mc_version is null
      or trim(p_mc_version) = ''
      or p.mc_version = trim(p_mc_version)
    )
  group by 1
  order by pack_count desc, partner asc
  limit greatest(coalesce(p_limit, 20), 1);
$$;

revoke all on function public.partners_for_mod(text, integer, text, text) from public;
grant execute on function public.partners_for_mod(text, integer, text, text) to anon, authenticated, service_role;

comment on function public.partners_for_mod(text, integer, text, text) is
  'Top N mods that co-occur with p_mod across pack observations (summed count).';
