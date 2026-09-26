-- Cached YouTube feed for launcher home (server-side crawl only).
-- Writes via Edge Function fetch-youtube-feed (service role).
-- Anon / authenticated clients may only SELECT.

create table if not exists public.youtube_feed (
  video_id text primary key,
  title text not null,
  thumbnail_url text not null,
  channel_name text not null,
  published_at timestamptz,
  view_count bigint not null default 0,
  fetched_at timestamptz not null default now(),
  query_tag text,
  constraint youtube_feed_video_id_nonempty check (char_length(trim(video_id)) > 0),
  constraint youtube_feed_title_nonempty check (char_length(trim(title)) > 0),
  constraint youtube_feed_thumb_nonempty check (char_length(trim(thumbnail_url)) > 0),
  constraint youtube_feed_channel_nonempty check (char_length(trim(channel_name)) > 0),
  constraint youtube_feed_view_count_nonneg check (view_count >= 0)
);

create index if not exists youtube_feed_view_count_idx
  on public.youtube_feed (view_count desc);

create index if not exists youtube_feed_fetched_at_idx
  on public.youtube_feed (fetched_at desc);

alter table public.youtube_feed enable row level security;

drop policy if exists youtube_feed_select_anon on public.youtube_feed;
create policy youtube_feed_select_anon
  on public.youtube_feed
  for select
  to anon, authenticated
  using (true);

grant select on public.youtube_feed to anon, authenticated;
revoke insert, update, delete on public.youtube_feed from anon, authenticated;
grant select, insert, update, delete on public.youtube_feed to service_role;

comment on table public.youtube_feed is
  'Hot Minecraft YouTube clips for launcher home. Filled by fetch-youtube-feed Edge Function; clients SELECT only.';
