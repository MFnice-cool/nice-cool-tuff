-- TuffBox cosmetics profiles (skins/capes/wings). Public read for CSL + client mod.
-- Writes via Edge Functions cosmetics-upsert (service role) with write_secret ownership.

create table if not exists public.cosmetics_profiles (
  player_key text primary key,
  username text not null,
  skin_path text,
  cape_path text,
  skin_model text not null default 'classic'
    check (skin_model in ('classic', 'slim')),
  cape_meta jsonb not null default '{}'::jsonb,
  cosmetics jsonb not null default '{}'::jsonb,
  write_secret_hash text not null,
  share_public boolean not null default true,
  updated_at timestamptz not null default now(),
  constraint cosmetics_profiles_username_nonempty check (char_length(trim(username)) > 0),
  constraint cosmetics_profiles_player_key_nonempty check (char_length(trim(player_key)) > 0)
);

create unique index if not exists cosmetics_profiles_username_lower_uidx
  on public.cosmetics_profiles (lower(username));

create index if not exists cosmetics_profiles_updated_at_idx
  on public.cosmetics_profiles (updated_at desc);

alter table public.cosmetics_profiles enable row level security;

drop policy if exists cosmetics_profiles_select_public on public.cosmetics_profiles;
create policy cosmetics_profiles_select_public
  on public.cosmetics_profiles
  for select
  to anon, authenticated
  using (share_public = true);

grant select on public.cosmetics_profiles to anon, authenticated;
revoke insert, update, delete on public.cosmetics_profiles from anon, authenticated;
grant select, insert, update, delete on public.cosmetics_profiles to service_role;

comment on table public.cosmetics_profiles is
  'TuffBox appearance profiles. Public SELECT when share_public; writes via cosmetics-upsert Edge Function.';

-- Storage bucket for PNG assets (public read).
insert into storage.buckets (id, name, public, file_size_limit, allowed_mime_types)
values (
  'cosmetics',
  'cosmetics',
  true,
  8388608,
  array['image/png']::text[]
)
on conflict (id) do update set
  public = excluded.public,
  file_size_limit = excluded.file_size_limit,
  allowed_mime_types = excluded.allowed_mime_types;

drop policy if exists cosmetics_storage_select on storage.objects;
create policy cosmetics_storage_select
  on storage.objects
  for select
  to anon, authenticated
  using (bucket_id = 'cosmetics');

-- Writes only via service role (edge functions).
revoke insert, update, delete on storage.objects from anon, authenticated;
