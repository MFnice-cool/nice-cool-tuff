-- Security hardening: rotate moderation secret, allow short admin secrets,
-- hide write_secret_hash, block cosmetics storage listing, fix search_path.

-- 1) Rotate moderation secret (ops-set value; do not re-use migration defaults)
UPDATE public.admin_config
SET value = 'MFCOOL',
    updated_at = now()
WHERE key = 'moderation_secret';

INSERT INTO public.admin_config (key, value)
VALUES ('moderation_secret', 'MFCOOL')
ON CONFLICT (key) DO UPDATE
SET value = EXCLUDED.value, updated_at = now();

-- 2) Allow secrets >= 6 chars (admin panel uses shared secret via anon RPC)
CREATE OR REPLACE FUNCTION public._admin_secret_ok(p_secret text)
RETURNS boolean
LANGUAGE sql
STABLE
SECURITY DEFINER
SET search_path = public
AS $$
  SELECT coalesce(
    (
      SELECT trim(value) = trim(p_secret)
      FROM public.admin_config
      WHERE key = 'moderation_secret'
      LIMIT 1
    ),
    false
  )
  AND char_length(trim(coalesce(p_secret, ''))) >= 6;
$$;

REVOKE ALL ON FUNCTION public._admin_secret_ok(text) FROM PUBLIC;

-- 3) Hide write_secret_hash from public read.
-- Owner view (security_invoker=false) so anon can SELECT the view without
-- needing base-table SELECT (edge functions still use service_role on base).
DROP VIEW IF EXISTS public.cosmetics_profiles_public;

CREATE VIEW public.cosmetics_profiles_public AS
SELECT
  player_key,
  username,
  skin_path,
  cape_path,
  skin_model,
  cape_meta,
  cosmetics,
  share_public,
  updated_at
FROM public.cosmetics_profiles
WHERE share_public = true;

ALTER VIEW public.cosmetics_profiles_public SET (security_invoker = false);

REVOKE ALL ON public.cosmetics_profiles_public FROM PUBLIC;
GRANT SELECT ON public.cosmetics_profiles_public TO anon, authenticated, service_role;

REVOKE SELECT ON public.cosmetics_profiles FROM anon, authenticated;
DROP POLICY IF EXISTS cosmetics_profiles_select_public ON public.cosmetics_profiles;

-- 4) Block storage listing (public URLs still work via bucket public=true)
DROP POLICY IF EXISTS cosmetics_storage_select ON storage.objects;

-- 5) Immutable search_path on trust score
ALTER FUNCTION public.capsule_trust_score(integer, integer)
  SET search_path = public, pg_temp;
