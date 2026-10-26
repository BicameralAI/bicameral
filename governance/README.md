# `governance/`

Machine-readable inputs to repository governance.

- [`structure.json`](structure.json) is the single authoritative description of the repository layout: allowed root files and directories, their purpose, child policy (`declared` or `open`), status (`active` or `reserved`) and code owners. `.github/CODEOWNERS` is generated from it.
- [`legacy-decisions.json`](legacy-decisions.json) locks the legacy decision directories that came in with component imports (`legacy_decision_dirs`). It holds each file's SHA-256. Any file added, edited or removed there fails the structure check. It is reviewed data, not a generated file: `--write` never updates it.

The rules are established by [ARCH-0002](../docs/architecture/records/ARCH-0002-repository-structure-and-structure-manifest.md) and enforced by [`tools/governance/check_structure.py`](../tools/governance/check_structure.py). Changes here are high-risk under [GOVERNANCE.md](../GOVERNANCE.md).
