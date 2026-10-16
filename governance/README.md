# `governance/`

Machine-readable inputs to repository governance.

- [`structure.json`](structure.json) is the single authoritative description of the repository layout: allowed root files and directories, their purpose, child policy (`declared` or `open`), status (`active` or `reserved`) and code owners. `.github/CODEOWNERS` is generated from it.

The rules are proposed in [ARCH-0002](../docs/architecture/records/ARCH-0002-repository-structure-and-structure-manifest.md) and enforced by [`tools/governance/check_structure.py`](../tools/governance/check_structure.py). Changes here are high-risk under [GOVERNANCE.md](../GOVERNANCE.md).
