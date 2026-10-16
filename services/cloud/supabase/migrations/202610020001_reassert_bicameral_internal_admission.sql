-- Alpha/demo invariant: authenticated @bicameral-ai.com identities must pass
-- the existing server-side design-partner admission gate.
--
-- This is intentionally a new migration rather than relying on the historical
-- seed migration. If an operator changed or disabled the row after that older
-- migration ran, replaying the current migration set must restore the internal
-- demo access invariant deterministically.
insert into public.design_partner_allowlist (domain, enabled, notes)
values (
  'bicameral-ai.com',
  true,
  'Bicameral internal domain; required for alpha/demo Cloud admission'
)
on conflict (domain) do update
set enabled = true,
    notes = excluded.notes,
    updated_at = now();
