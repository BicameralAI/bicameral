-- qor:service-role-only — service-role backend reads this table; admins edit it in Supabase (see table comment below).
create table if not exists public.design_partner_allowlist (
  id uuid primary key default gen_random_uuid(),
  domain text not null unique,
  enabled boolean not null default true,
  notes text,
  created_at timestamptz not null default now(),
  updated_at timestamptz not null default now(),
  constraint design_partner_allowlist_domain_normalized
    check (domain = lower(trim(leading '@' from domain)))
);

create index if not exists design_partner_allowlist_domain_enabled_idx
  on public.design_partner_allowlist (domain)
  where enabled;

alter table public.design_partner_allowlist enable row level security;

create or replace function public.set_updated_at()
returns trigger
language plpgsql
as $$
begin
  new.updated_at = now();
  return new;
end;
$$;

drop trigger if exists set_design_partner_allowlist_updated_at
  on public.design_partner_allowlist;

create trigger set_design_partner_allowlist_updated_at
before update on public.design_partner_allowlist
for each row execute function public.set_updated_at();

comment on table public.design_partner_allowlist is
  'Design partner phase domain allowlist. Service-role backend reads this table; admins edit it in Supabase.';

comment on column public.design_partner_allowlist.domain is
  'Lowercase email domain. Omit the leading @.';
