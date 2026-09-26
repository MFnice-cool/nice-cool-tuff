-- Popular vs creator source + language tag for client-side locale filter.

alter table public.youtube_feed
  add column if not exists source text not null default 'popular';

alter table public.youtube_feed
  add column if not exists lang text not null default 'en';

alter table public.youtube_feed
  drop constraint if exists youtube_feed_source_check;

alter table public.youtube_feed
  add constraint youtube_feed_source_check
  check (source in ('popular', 'channel'));

alter table public.youtube_feed
  drop constraint if exists youtube_feed_lang_check;

alter table public.youtube_feed
  add constraint youtube_feed_lang_check
  check (char_length(lang) >= 2 and char_length(lang) <= 8);

create index if not exists youtube_feed_lang_source_views_idx
  on public.youtube_feed (lang, source, view_count desc);

comment on column public.youtube_feed.source is
  'popular = keyword search hits; channel = tracked creator uploads';
comment on column public.youtube_feed.lang is
  'ISO 639-1 (or BCP47 primary) of the crawl / video audio language';
