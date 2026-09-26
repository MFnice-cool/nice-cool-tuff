-- Release hardening: the shared moderation secret was previously shipped in
-- this repository (migration 015 set it to a literal value). Anyone who read
-- the public repo therefore held the admin/moderation credential for the
-- community backend. Fail closed: clear the stored secret and reject the
-- known compromised default even if re-inserted. Ops MUST set a strong
-- secret out-of-band before using any moderation RPC:
--
--   UPDATE public.admin_config SET value = '<long random secret>'
--   WHERE key = 'moderation_secret';
--
-- The desktop app never embeds this secret (verified), so clearing it breaks
-- no shipped functionality — only un-provisioned admin access.

UPDATE public.admin_config
SET value = '',
    updated_at = now()
WHERE key = 'moderation_secret'
  AND (
    value IS NULL
    OR length(trim(value)) < 16
    OR lower(trim(value)) IN ('mfcool', 'changeme', 'secret', 'admin', 'password')
  );

-- Defense in depth: never accept the compromised default again, even if a
-- stale migration or manual insert brings it back.
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
  AND char_length(trim(coalesce(p_secret, ''))) >= 16
  AND lower(trim(coalesce(p_secret, ''))) <> 'mfcool';
$$;

REVOKE ALL ON FUNCTION public._admin_secret_ok(text) FROM PUBLIC;
