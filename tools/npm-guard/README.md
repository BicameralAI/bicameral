# npm-guard

Supply-chain protections for the npm project in this repository (`apps/bot/dashboard`). Tracked in #92.

npm runs dependency install scripts by default. A malicious, typosquatted or hijacked package can use
them to run code on a developer machine or a CI runner. Some campaigns also inject a loader into project
files. The loader runs on the next `npm run dev` or `npm run build`, and it spreads through `git pull` and
`git push`.

These checks are limited protections. The scanner finds known signatures and patterns only. A clean
result is not proof that the code is clean.

## Layers

| Layer | What it does | Where |
|---|---|---|
| `.npmrc` | `ignore-scripts=true`: dependency install scripts do not run. `min-release-age=7`: npm refuses versions published less than 7 days ago (npm 11.10+). | `apps/bot/dashboard/.npmrc` |
| Run-time scan | Every dashboard script runs `gate.cjs --gate` first, and refuses to start if a file in the tree has a malware signature. | `apps/bot/dashboard/package.json` |
| Git hooks | `pre-commit` and `pre-push` block malware signatures and risky dependency changes. `pre-merge-commit` blocks an infected merge. `post-merge`, `post-rewrite` and `post-checkout` warn after the fact. | `hooks/` |
| `git safe-pull` | Fetches, scans the incoming commits, and merges only if they are clean. | `safe-pull.sh` |
| Claude Code hook | Vets `npm`/`pnpm`/`yarn`/`bun` `install` and `add` commands that the assistant runs. Refuses Node commands while the tree is infected. | `guard_npm_install.py`, `/.claude/settings.json` |
| CI | Scan, guard verification, dependency-change check, guard tests, `npm ci --ignore-scripts` and `npm audit signatures`. | `/.github/workflows/npm-supply-chain.yml` |

### Why the scan runs inline, not in a `pre*` script

`ignore-scripts=true` also turns off npm `pre` and `post` scripts. A `prebuild` or `pretest` guard does
not run at all. Each dashboard script therefore starts with the guard:

```json
"test": "node ../../../tools/npm-guard/gate.cjs --gate && vitest run"
```

`tests/test_npm_guard.py` holds the negative control: with `ignore-scripts=true`, a failing `pre` script is
skipped, and the inline guard still refuses.

### How CI trusts the guard

For a pull request, CI runs the guard code from the **base branch** against the files of the pull
request. A pull request cannot weaken the scanner that checks it. CI calls the scanner with `python3`
directly, so a missing interpreter or scanner fails the job. Local hooks are a convenience. CI does not
depend on them.

## Setup (once per clone)

Requirements: `git`, Python 3.6+ (standard library only), Node.js, and npm 11.10+ for the release-age
cooldown. On Windows, use Git for Windows.

```sh
cd apps/bot/dashboard
npm run security:setup    # enable the git hooks and the git safe-pull alias for this clone
npm run security:check    # every line must say "ok"
```

Setup changes only this clone's git config. It does not change your global npm config. If you already
use another `core.hooksPath`, setup does not overwrite it. Call the hooks in `hooks/` from your own hooks.

## What the dependency check blocks

On commit, on push and in CI, a change to `package.json` or `package-lock.json` fails when it:

- adds a git, URL or GitHub-shorthand dependency
- adds or changes a `preinstall`, `install` or `postinstall` script
- changes dependencies without a lockfile change in the same directory
- adds a package that does not exist on the registry, is less than 7 days old, or has fewer than 1000
  weekly downloads
- adds a lockfile `resolved` URL outside `registry.npmjs.org`

It only warns when the registry is unreachable.

## Exceptions

| Need | How |
|---|---|
| Accept a reviewed package that the dependency check flags | Add its name to `allow.txt`, with the reason. Code owners review the entry in the pull request. CI reads `allow.txt` from the pull request. |
| Accept a package for one local commit | `NPM_GUARD_ALLOW=name git commit ...` (CI checks again) |
| Install an urgent security fix that is less than 7 days old | `npm install <pkg>@<fixed version> --min-release-age=0`, then open a pull request that links the advisory. The code-owner review of that pull request is the approval. `npm ci` installs from the reviewed lockfile and is not affected by the cooldown. |
| A reviewed false positive in the scanner | `NPM_GUARD_SCAN_ALLOW=path/to/file` for one local run |

## If the scan fires after a pull

Do not run the project. Undo the update with `git reset --hard ORIG_HEAD`, tell the maintainers through
the process in `SECURITY.md`, and rotate your tokens, SSH keys and `.env` secrets if you already ran it.

## Run the checks by hand

```sh
python3 tools/npm-guard/scan_incoming.py --worktree     # malware scan of the working tree
node tools/npm-guard/gate.cjs --verify                  # guard files and .npmrc intact, then scan
python3 -m unittest discover -s tools/npm-guard/tests -v  # guard tests (needs git, node, npm)
```

## Limits

- The Claude Code hook sees only commands that Claude Code runs. Commands you type yourself are covered by
  `.npmrc`, the git hooks and CI.
- The registry checks are heuristics (existence, age, popularity). They are not a malware scan of the
  package. A compromised established package is covered only by `ignore-scripts` and the cooldown.
- Running `vite` or `node` directly, without `npm run`, skips the run-time scan.
- `git commit --no-verify` and `git push --no-verify` skip the local hooks. CI is the backstop.
