# Product join-ticket signing compatibility fixture

`signed-product-join-ticket.valid.json` is a **test-only** signing fixture. The
ticket object mirrors the bot contract fixture
`protocol/onboarding/fixtures/product-join-ticket.valid.json`; the deterministic
Ed25519 envelope uses public test seed bytes `01..20` (hex) exclusively. It is
not a production credential.

`tests/test_product_join_ticket.py` proves Cloud's canonical signing bytes
verify with the bot runtime's raw-key/base64 Ed25519 transport contract and that
the ticket claim set stays fail-closed.

There is **no shadow canonical schema here**. Structural authority is the pinned,
fingerprint-verified bot schema vendored under
`src/bicameral_cloud/contracts/onboarding/schemas/`, loaded via
`bicameral_cloud.contracts.onboarding.load_schema`.
