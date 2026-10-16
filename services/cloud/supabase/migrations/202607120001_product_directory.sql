-- Durable hosted Product directory (cloud#125).
--
-- qor:service-role-only — the Cloud backend reads and writes this table with the
-- Supabase service-role key only. It is a NON-CANONICAL coordination/projection
-- table for Product discovery and routing; it is NOT canonical Product state.
-- The bot/daemon remains authoritative for Product creation, membership, roles,
-- policy, and events (ADR-0005/0006). This table must never hold canonical
-- Product membership/roles, Governance Policy state, provider credentials, raw
-- event-store descriptors, or local filesystem paths.
create table if not exists public.product_directory (
  id uuid primary key default gen_random_uuid(),
  -- Canonical Product identifier returned by the daemon's successful creation.
  -- Unique so publication is idempotent and cannot silently create duplicates.
  canonical_product_id text not null unique,
  display_name text not null,
  -- Verified email domain eligible to discover this Product (domain discovery
  -- scope). Lowercase, no leading '@'.
  eligible_domain text not null,
  -- Opaque daemon creation receipt reference. NOT a path/secret. Unique so a
  -- given registration cannot be silently remapped onto a different canonical
  -- Product ID.
  registration_ref text unique,
  -- Allowlisted, non-secret provider/substrate routing hints.
  provider_hint text,
  substrate_hint text,
  created_at timestamptz not null default now(),
  updated_at timestamptz not null default now(),
  constraint product_directory_display_name_not_empty
    check (length(trim(display_name)) > 0),
  constraint product_directory_eligible_domain_normalized
    check (eligible_domain = lower(trim(leading '@' from eligible_domain)))
);

create index if not exists product_directory_eligible_domain_idx
  on public.product_directory (eligible_domain);

alter table public.product_directory enable row level security;

-- Fail-closed: only the service-role backend (which bypasses RLS) may touch this
-- table. No RLS policies are defined, so anon/authenticated browser roles get no
-- access even if they reach PostgREST. Revoke is explicit and defense-in-depth.
-- Guarded so the migration also applies cleanly on a plain Postgres database
-- that lacks the Supabase-managed browser roles.
do $$
begin
  if exists (select 1 from pg_roles where rolname = 'anon') then
    execute 'revoke all on public.product_directory from anon';
  end if;
  if exists (select 1 from pg_roles where rolname = 'authenticated') then
    execute 'revoke all on public.product_directory from authenticated';
  end if;
end
$$;

-- Reuse the shared updated_at trigger function from the allowlist migration.
drop trigger if exists set_product_directory_updated_at
  on public.product_directory;

create trigger set_product_directory_updated_at
before update on public.product_directory
for each row execute function public.set_updated_at();

comment on table public.product_directory is
  'Non-canonical hosted Product directory for discovery/routing (cloud#125). Service-role backend only; never canonical Product state, membership, roles, policy, credentials, event-store descriptors, or local paths.';

comment on column public.product_directory.canonical_product_id is
  'Canonical Product ID returned by the daemon on creation. Unique; publication is idempotent on this id.';

comment on column public.product_directory.eligible_domain is
  'Lowercase verified email domain eligible to discover this Product. Omit the leading @.';

comment on column public.product_directory.registration_ref is
  'Opaque daemon creation-receipt reference. Unique; prevents silently remapping a registration onto a different canonical Product ID. Never a path or secret.';
