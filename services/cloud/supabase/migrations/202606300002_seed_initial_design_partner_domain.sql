insert into public.design_partner_allowlist (domain, enabled, notes)
values ('bicameral-ai.com', true, 'initial design partner domain')
on conflict (domain) do update
set enabled = excluded.enabled,
    notes = excluded.notes,
    updated_at = now();
