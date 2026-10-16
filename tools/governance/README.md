# `tools/governance/`

Validators that enforce the repository's governance rules. They use the Python 3.10+ standard library and Git only, so they run anywhere without installing anything.

| File | Purpose |
| --- | --- |
| [`check.py`](check.py) | Runs every check below; this is what CI runs |
| [`check_structure.py`](check_structure.py) | Validates [`governance/structure.json`](../../governance/structure.json), file placement and the generated `.github/CODEOWNERS` |
| [`check_architecture.py`](check_architecture.py) | Validates architecture records and the generated architecture index |
| [`tests/`](tests/) | Unit tests; every fixture runs in a temporary Git repository |

```bash
python3 tools/governance/check.py          # validate; never modifies the working tree
python3 tools/governance/check.py --write  # regenerate generated files, then validate
```
