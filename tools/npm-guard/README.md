# npm-guard

Supply-chain protections for the npm project in this repository (`apps/bot/dashboard`). Tracked in #92.

npm runs dependency install scripts by default. A malicious, typosquatted or hijacked package can use
them to run code on a developer machine or a CI runner. Some campaigns also inject a loader into project
files. The loader runs on the next `npm run dev` or `npm run build`, and it spreads through `git pull` and
`git push`.

These checks are limited protections. The scanner finds known signatures and patterns only. A finding
or a clean result is an indicator, not proof. Registry signatures (`npm audit signatures`) prove where a
package came from, not that its publisher is benign.

The enforced layer is CI: a required check on protected `main` that runs the guard code from the base
branch. The local layers (git hooks, `git safe-pull`, the run-time scan) are optional, and a contributor
can bypass them.

## Layers

| Layer | What it does | Where |
|---|---|---|
| `.npmrc` | `ignore-scripts=true`: dependency install scripts do not run. `min-release-age=7`: npm refuses versions published less than 7 days ago (npm 11.10+). | `apps/bot/dashboard/.npmrc` |
| Run-time scan | Every dashboard script runs `gate.cjs --gate` first. It refuses to start if a file in the tree has a malware signature, or if Python 3 or the scanner is missing. | `apps/bot/dashboard/package.json` |
| Git hooks | `pre-commit` and `pre-push` block malware signatures and risky dependency changes. `pre-merge-commit` blocks an infected merge. `post-merge`, `post-rewrite` and `post-checkout` warn after the fact. | `hooks/`, installed into `.git/npm-guard` |
| `git safe-pull` | Fetches, scans the incoming commits, and merges only if they are clean. | `safe-pull.sh` |
| CI | Scan, guard verification, dependency-change check, guard tests, `npm ci --ignore-scripts` and `npm audit signatures`. A Windows job runs setup, the gate and the dashboard build. | `/.github/workflows/npm-supply-chain.yml` |

### What the scanner reads

The scanner reads every file as bytes, whatever its name or type. It never runs or imports them.

| Scan | Files |
|---|---|
| `--worktree` (run-time scan, CI) | Every tracked file, and every untracked file that `.gitignore` does not exclude. Ignored paths such as `node_modules/` and `dist/` are not scanned. |
| `--staged`, `--range`, `--ref` (git hooks, `git safe-pull`) | Every file that the commit, push or merge adds or changes. |

It fails closed: a file over 5 MiB, a file it cannot read, and a symbolic link to a directory are
reported as findings. In the worktree scan, a symbolic link to a file is scanned through to its target,
also outside the repository, because Node follows the link. The diff scans read what git stores for a
link, which is the target path, so the worktree scan is the one that checks the target.

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

Bootstrap: until the guard is on `main`, CI has no base-branch copy and runs the copy from the pull
request. The first version is therefore trusted through independent review, not through CI.

## Setup (once per clone)

Requirements: `git`, Python 3.6+ (standard library only), Node.js, and npm 11.10+ for the release-age
cooldown. The dashboard scripts do not start without Python 3. On Windows, use Git for Windows (setup
needs its `sh`) and Python 3 from python.org (`python` or `py -3`).

```sh
cd apps/bot/dashboard
npm run security:setup    # enable the git hooks and the git safe-pull alias for this clone
npm run security:check    # every line must say "ok"
```

Setup copies the hooks and the scripts they call into `.git/npm-guard`, and points `core.hooksPath` and
`git safe-pull` there. Git runs a hook after it updates your files. A hook that ran from the working tree
would therefore run code from any branch you check out or pull. The installed copy changes only when you
run setup again. After `tools/npm-guard` changes, run setup again on a reviewed `main`.
`npm run security:check` reports when the installed copy is out of date.

Setup changes only this clone's git config. It does not change your global npm config. If you already
use another `core.hooksPath`, setup stops and does not overwrite it.

## What the dependency check blocks

On commit, on push and in CI, a change to `package.json` or `package-lock.json` fails when it:

- adds a git, URL or GitHub-shorthand dependency
- adds or changes a `preinstall`, `install` or `postinstall` script
- changes dependencies without a lockfile change in the same directory
- adds a package that does not exist on the registry, is less than 7 days old, or has fewer than 1000
  weekly downloads (a pinned version is judged by its own age, not by the age of the latest release)
- adds or bumps a lockfile entry, direct or transitive, to a version less than 7 days old, or to a
  package that does not exist
- adds a lockfile `resolved` URL outside `registry.npmjs.org`
- changes a lockfile `integrity` value while the version and URL stay the same

It only warns when the registry or the download-count service is unreachable.

`gate.cjs --verify` (in CI) also fails when the dashboard `.npmrc` turns `ignore-scripts` off, sets
`min-release-age` below 7, or sets `registry`, a scoped `registry` or `replace-registry-host`. A later
line counts, as it does for npm.

## Exceptions

| Need | How |
|---|---|
| Accept a reviewed package that the dependency check flags | Add its name to `allow.txt`, with the reason. Code owners review the entry in the pull request. CI reads `allow.txt` from the pull request. |
| Accept a package for one local commit | `NPM_GUARD_ALLOW=name git commit ...` (CI checks again) |
| Install an urgent security fix that is less than 7 days old | `npm install <pkg>@<fixed version> --min-release-age=0`. Add the package to `allow.txt` with the advisory as the reason, and link the advisory in the pull request. The code-owner review of that pull request is the approval. Remove the entry once the version is 7 days old. `npm ci` installs from the reviewed lockfile and is not affected by the cooldown. |
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

- The git hooks are optional. A clone without `npm run security:setup`, `git commit --no-verify`,
  `git push --no-verify` or a changed `core.hooksPath` skips them. CI is the backstop.
- `git safe-pull` is a separate command. It does not wrap `git pull`, and a plain `git pull` updates your
  files before any scan runs. The `post-merge` hook then only warns.
- The registry checks are heuristics (existence, age, popularity). They are not a malware scan of the
  package. A compromised established package is covered only by `ignore-scripts` and the cooldown.
- Running `vite` or `node` directly, without `npm run`, skips the run-time scan.
- The scanner does not read `node_modules/` or other ignored paths. Installed packages are covered only by
  `ignore-scripts`, the cooldown, the dependency check and `npm audit signatures`.
