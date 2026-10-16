-- Product join-ticket eligibility control (cloud#126).
--
-- This flag only controls Cloud ticket issuance from the non-canonical Product
-- directory. It creates no membership or pending-approval state; the daemon
-- remains authoritative for Product policy and membership.
alter table public.product_directory
  add column if not exists enabled boolean not null default true;

comment on column public.product_directory.enabled is
  'Whether Cloud may issue an ephemeral join ticket for this non-canonical directory row. The daemon still re-evaluates canonical join policy.';
