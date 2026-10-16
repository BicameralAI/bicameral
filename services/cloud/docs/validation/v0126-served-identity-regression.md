# v0.1.26 served-identity provenance regression

Cloud #272 demonstrated that self-consistent Cloud-authored release metadata is not sufficient proof that the hosted dashboard bytes came from the declared Bot commit.

The negative fixture is the exact failure class from v0.1.26: the checked-in Bot release descriptor and dashboard version metadata may be relabeled to a newer Bot commit while retaining an older `hosted_spa` digest and older vendored SPA bytes. Such a state must fail before deployment can become publication evidence.

The repaired verification chain is:

`Bot commit -> immutable Bot dashboard tarball -> embedded Bot release descriptor -> embedded dashboard/dist bytes -> Cloud vendored dashboard bytes -> served release identity`

The immutable Bot tarball is fetched from the commit-keyed producer artifact path and its SHA-256 must match the digest recorded by the vendor workflow. The descriptor embedded in that tarball must match the checked-in Cloud descriptor input exactly, including the Bot commit and descriptor digest. The `dashboard/dist` bytes in the immutable artifact must produce the same tree digest as the Cloud-vendored SPA. No Cloud-recomputed descriptor, commit label, or version field can substitute for those comparisons.

PR #275 is the cumulative `dev`-targeting recovery candidate. Its head contains the real immutable vendor bytes from PR #273 plus this provenance verifier, so normal Cloud release/provenance and terminal validation evaluate one exact integration head rather than splitting correctness across two PR bases.

Passing this verification is release-provenance evidence only. It does not grant merge, deployment, publication, Product, Decision, or acceptance authority.
