-- Bind every new Product directory publication to the validated
-- provider-confirmed CreateProduct witness. Existing pre-v0.1.14 rows remain
-- nullable and require reconciliation; the API requires this field for every
-- new write.
alter table public.product_directory
  add column if not exists creation_commit_proof_digest text;

alter table public.product_directory
  add column if not exists creation_event_id uuid;

alter table public.product_directory
  add column if not exists creation_event_digest text;

alter table public.product_directory
  drop constraint if exists product_directory_creation_commit_proof_digest_check;

alter table public.product_directory
  add constraint product_directory_creation_commit_proof_digest_check
  check (
    creation_commit_proof_digest is null
    or creation_commit_proof_digest ~ '^sha256:[0-9a-f]{64}$'
  );

alter table public.product_directory
  drop constraint if exists product_directory_creation_event_digest_check;

alter table public.product_directory
  add constraint product_directory_creation_event_digest_check
  check (
    creation_event_digest is null
    or creation_event_digest ~ '^sha256:[0-9a-f]{64}$'
  );
