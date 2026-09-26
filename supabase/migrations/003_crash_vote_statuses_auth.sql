-- Status rename + auth-required votes (voter_user_id).
-- Applied remotely via Supabase MCP; kept in repo for history.

update public.experience_capsules set status = 'open' where status = 'pending';
update public.experience_capsules set status = 'saved' where status = 'active';
update public.experience_capsules set status = 'rejected' where status = 'quarantined';

alter table public.experience_capsules drop constraint if exists experience_capsules_status_check;
alter table public.experience_capsules
  alter column status set default 'open';
alter table public.experience_capsules
  add constraint experience_capsules_status_check
  check (status in ('open', 'saved', 'rejected'));

alter table public.capsule_votes drop constraint if exists capsule_votes_pkey;

alter table public.capsule_votes
  alter column voter_public_key drop not null;
alter table public.capsule_votes
  alter column signature drop not null;

alter table public.capsule_votes
  add column if not exists voter_user_id uuid references auth.users(id) on delete cascade;

delete from public.capsule_votes where voter_user_id is null;

alter table public.capsule_votes
  alter column voter_user_id set not null;

alter table public.capsule_votes
  add constraint capsule_votes_pkey primary key (content_hash, voter_user_id);

create index if not exists capsule_votes_user_idx on public.capsule_votes (voter_user_id);

drop policy if exists experience_capsules_select_anon on public.experience_capsules;
create policy experience_capsules_select_anon
  on public.experience_capsules
  for select
  to anon, authenticated
  using (status in ('open', 'saved'));

drop policy if exists capsule_votes_deny_all on public.capsule_votes;
create policy capsule_votes_deny_all
  on public.capsule_votes
  for all
  to anon, authenticated
  using (false)
  with check (false);
