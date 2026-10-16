# Google Desktop credential secret boundary

Cloud #309 adds the authenticated `GET /api/v1/provider-app-credential/google` bootstrap seam required by the frozen Row 5 contract.

## Runtime setting

The Cloud process reads the provider-required Google Desktop credential only from:

`BICAMERAL_GOOGLE_OAUTH_CLIENT_SECRET`

This value is secret material. It must be mounted into Cloud Run from the approved Secret Manager/release boundary. It must not be stored as a repository variable, committed to source, added to the public `/api/v1/provider-app-config/google` payload, or included in logs/evidence.

The governed Secret Manager resource names are:

- production: `google-oauth-desktop-client-secret-prod`
- staging: `google-oauth-desktop-client-secret-staging`

These names match the existing Secret Manager resources in project `bicameral-cloud-500911`; the deploy contract must reference the canonical resources rather than create duplicate aliases. The deploy workflow maps the environment variable to the environment-specific Secret Manager resource at `latest`. Secret creation and value rotation remain production-authority actions; the repository contains only the resource names and mount contract, never the credential value.

## Binding

The authenticated credential response returns the credential together with the exact public application binding:

- `client_id`
- `environment`
- `config_version`

The daemon must exact-match all three values against the already validated public app config before storing or using the credential.

## Authentication boundary

The endpoint uses the existing Cloud bearer authentication dependency. The signed-in operator's transient Cloud bearer is request authority for this fetch only. It is not Google token material, is not daemon identity enrollment, and must not be persisted as provider state.

## Production provisioning

Production deployment must mount `BICAMERAL_GOOGLE_OAUTH_CLIENT_SECRET` from Secret Manager resource `google-oauth-desktop-client-secret-prod` before Row 5 can be admitted for real-provider acceptance. Missing secret material fails closed with HTTP 503; there is no production fallback to manual `BICAMERAL_GOOGLE_CLIENT_*` injection.

Secret creation/value rotation remains a production-authority action. Cloud Run mounting is governed by the deployment workflow and must not be performed through plaintext repository configuration. The implementation contains no credential value.
