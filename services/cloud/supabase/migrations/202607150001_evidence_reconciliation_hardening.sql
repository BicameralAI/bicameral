-- Revoke-by-default hardening for the public schema (cloud#141, prep for the
-- Cloud #106 evidence gate).
--
-- This migration codifies the repository-known, fail-closed principal posture that
-- does NOT require the owner-only live export to identify:
--
--   * browser principals (anon, authenticated) hold NO privileges on public by
--     default, now and for future objects (default privileges);
--   * the service-role backend keeps its access (it bypasses RLS and is the only
--     writer of the non-canonical coordination tables — ADR-0005/0006);
--   * RLS is enabled AND forced on the existing coordination tables so a future
--     accidental grant still cannot expose rows without an explicit policy.
--
-- It is idempotent and guarded on role existence so it applies cleanly whether or
-- not the Supabase auth roles are provisioned in a given environment.

-- 1. Remove any ambient privileges browser principals may have on the schema and
--    on every existing object in it.
do $$
begin
  if exists (select 1 from pg_roles where rolname = 'anon') then
    execute 'revoke all on schema public from anon';
    execute 'revoke all on all tables in schema public from anon';
    execute 'revoke all on all sequences in schema public from anon';
    execute 'revoke all on all functions in schema public from anon';
  end if;
  if exists (select 1 from pg_roles where rolname = 'authenticated') then
    execute 'revoke all on schema public from authenticated';
    execute 'revoke all on all tables in schema public from authenticated';
    execute 'revoke all on all sequences in schema public from authenticated';
    execute 'revoke all on all functions in schema public from authenticated';
  end if;
end
$$;

-- 2. Fail-closed for FUTURE objects: default privileges so a later migration that
--    forgets to revoke cannot silently grant anon/authenticated access.
do $$
begin
  if exists (select 1 from pg_roles where rolname = 'anon') then
    execute 'alter default privileges in schema public revoke all on tables from anon';
    execute 'alter default privileges in schema public revoke all on sequences from anon';
    execute 'alter default privileges in schema public revoke all on functions from anon';
  end if;
  if exists (select 1 from pg_roles where rolname = 'authenticated') then
    execute 'alter default privileges in schema public revoke all on tables from authenticated';
    execute 'alter default privileges in schema public revoke all on sequences from authenticated';
    execute 'alter default privileges in schema public revoke all on functions from authenticated';
  end if;
end
$$;

-- 3. Defense in depth: enable AND force RLS on the existing coordination tables so
--    even the table owner is subject to RLS. The service-role backend bypasses RLS
--    and is unaffected; browser principals have no policy and therefore no rows.
alter table if exists public.design_partner_allowlist enable row level security;
alter table if exists public.design_partner_allowlist force row level security;

alter table if exists public.product_directory enable row level security;
alter table if exists public.product_directory force row level security;

comment on schema public is
  'Revoke-by-default: anon/authenticated hold no privileges on public (current and future objects). The service-role backend is the only browser-reachable writer, and it bypasses RLS (cloud#141).';
