-- In-game overlay social layer: presence, friendships, direct chat.
-- All access goes through Edge Functions (service role) with writeSecret
-- ownership validated against cosmetics_profiles (same credential as
-- cosmetics-upsert). Direct table access is denied for anon.

create table if not exists public.player_presence (
  player_key text primary key,
  username text not null,
  pack_name text not null default '',
  server text not null default '',
  updated_at timestamptz not null default now(),
  constraint player_presence_player_key_nonempty check (char_length(trim(player_key)) > 0),
  constraint player_presence_username_nonempty check (char_length(trim(username)) > 0)
);

create index if not exists player_presence_updated_at_idx
  on public.player_presence (updated_at desc);

create table if not exists public.player_friendships (
  id bigint generated always as identity primary key,
  requester_key text not null,
  requester_name text not null,
  addressee_key text not null,
  addressee_name text not null,
  status text not null default 'pending'
    check (status in ('pending', 'accepted')),
  created_at timestamptz not null default now(),
  updated_at timestamptz not null default now(),
  constraint player_friendships_pair_unique unique (requester_key, addressee_key),
  constraint player_friendships_no_self check (requester_key <> addressee_key)
);

create index if not exists player_friendships_addressee_idx
  on public.player_friendships (addressee_key, status);
create index if not exists player_friendships_requester_idx
  on public.player_friendships (requester_key, status);

create table if not exists public.chat_messages (
  id bigint generated always as identity primary key,
  conversation_id text not null,
  from_key text not null,
  from_name text not null,
  to_key text not null,
  body text not null,
  created_at timestamptz not null default now(),
  constraint chat_messages_body_len check (char_length(body) between 1 and 500)
);

create index if not exists chat_messages_conversation_idx
  on public.chat_messages (conversation_id, id);
create index if not exists chat_messages_to_key_idx
  on public.chat_messages (to_key, id);

-- Retention: drop messages older than 30 days (called from overlay-chat-poll, throttled).
create or replace function public.chat_messages_prune()
returns bigint
language sql
security definer
set search_path = public
as $$
  with deleted as (
    delete from public.chat_messages
    where created_at < now() - interval '30 days'
    returning id
  )
  select count(*)::bigint from deleted;
$$;

alter table public.player_presence enable row level security;
alter table public.player_friendships enable row level security;
alter table public.chat_messages enable row level security;

drop policy if exists player_presence_deny_direct on public.player_presence;
create policy player_presence_deny_direct
  on public.player_presence for all to anon, authenticated
  using (false) with check (false);

drop policy if exists player_friendships_deny_direct on public.player_friendships;
create policy player_friendships_deny_direct
  on public.player_friendships for all to anon, authenticated
  using (false) with check (false);

drop policy if exists chat_messages_deny_direct on public.chat_messages;
create policy chat_messages_deny_direct
  on public.chat_messages for all to anon, authenticated
  using (false) with check (false);

revoke all on table public.player_presence from anon, authenticated;
revoke all on table public.player_friendships from anon, authenticated;
revoke all on table public.chat_messages from anon, authenticated;

grant select, insert, update, delete on public.player_presence to service_role;
grant select, insert, update, delete on public.player_friendships to service_role;
grant select, insert, update, delete on public.chat_messages to service_role;

comment on table public.player_presence is
  'In-game overlay presence (heartbeat ~30s; stale >2min treated as offline). Writes via overlay-presence Edge Function.';
comment on table public.player_friendships is
  'TuffBox friend relationships for the in-game overlay. Access via overlay-friends Edge Function.';
comment on table public.chat_messages is
  'In-game overlay direct messages. Access via overlay-chat-send / overlay-chat-poll Edge Functions.';
