#!/usr/bin/env python3
"""Detect known injected-loader code in git history before it can run on this machine.

The malware is JavaScript (usually appended to a *.config.js file) that starts with
`global.o=...` / `global['_V']=...`. It only does damage once somebody runs `npm run dev`,
`npm run build` or any tool that loads the config. This scanner reads git objects and files
as bytes; it never executes or imports anything from the repository.

  --range OLD NEW   files changed between two revisions (post-merge, post-checkout, pre-push)
  --ref REF         what merging/pulling REF would bring in (pre-merge, safe-pull)
  --staged          what is about to be committed (pre-commit); strict, any hit blocks
  --worktree        every tracked source file as it is on disk right now (gate before dev/build)

Pull-side scans (--ref, --range) flag a file only when the signature or an IOC appears in NEW and not
in OLD (so an old, already-reviewed hit is not re-reported on every pull). Scans of code leaving this
machine (--staged, --range --strict) and --worktree flag ANY hit in the files they cover.

Exit 0 clean, 1 malware found. Reviewed false positive: NPM_GUARD_SCAN_ALLOW=path1,path2.
These are signature checks for known campaigns. A clean result is not proof that the code is clean.
"""
import argparse
import os
import re
import subprocess
import sys

EMPTY_TREE = '4b825dc642cb6eb9a060e54bf8d69288fbee4904'
MAX_BYTES = 5 * 1024 * 1024
SOURCE_SUFFIXES = ('.js', '.cjs', '.mjs', '.jsx', '.ts', '.tsx', '.cts', '.mts',
                   '.json', '.woff', '.woff2', '.ttf', '.otf', '.bat', '.sh')
SOURCE_NAMES = ('.postcssrc', '.babelrc', '.eslintrc', '.gitignore')

MALICIOUS_SIGNATURE = re.compile(
    rb"(?<![\w$.])global(?:This)?\s*"
    rb"(?:\.\s*o|\[\s*(?:'o'|\"o\"|`o`)\s*\])"
    rb"(?![\w$])\s*=(?!=)"
)
CAMPAIGN_IOC = re.compile(
    rb"(?:global\s*\[\s*['\"](?:!|_V|r)['\"]\s*\]|_\$_1e42|rmcej%otb%|Cot%3t=shtP|"
    rb"A(?:4-1928|10-(?:010|2340|83-10))|"
    rb"api\.trongrid\.io|bsc-dataseed\.binance\.org|bsc-rpc\.publicnode\.com|"
    rb"TMfKQEd7TJJa5xNZJZ2Lep838vrzrs7mAP|TXfxHUet9pJVU1BgVkBAbrES4YUc1nGzcG|"
    rb"166\.88\.(?:54\.158|4\.2|134\.82)|198\.105\.127\.210|"
    rb"23\.27\.(?:202\.27|120\.142)|154\.91\.0\.103|136\.0\.9\.8|"
    rb"202\.155\.8\.173|188\.43\.33\.249)",
    re.I,
)
HIDDEN_SUFFIX = re.compile(rb"[^\n]{0,500};[ \t]{200,}\S")  # payload pushed off-screen after 200+ spaces
CHECKS = (('global.o signature', MALICIOUS_SIGNATURE), ('campaign IOC', CAMPAIGN_IOC),
          ('code hidden after 200+ spaces', HIDDEN_SUFFIX))


def git(*args, raw=False):
    try:
        result = subprocess.run(['git', *args], capture_output=True)
    except OSError:  # git not installed (bare Docker image): callers fall back
        return None
    if result.returncode != 0:
        return None
    return result.stdout if raw else result.stdout.decode('utf-8', 'replace')


def is_source(path):
    base = os.path.basename(path)
    return path.lower().endswith(SOURCE_SUFFIXES) or base in SOURCE_NAMES or base.startswith('.postcssrc')


def hits(data):
    if data is None:
        return {}
    if len(data) > MAX_BYTES:  # too big to scan: never treat it as clean
        return {'source file over 5 MiB, not scanned (review it)': 1}
    return {name: len(rx.findall(data)) for name, rx in CHECKS if rx.search(data)}


def blob(rev, path):
    return git('show', f'{rev}:{path}' if rev else f':{path}', raw=True)


def resolve(rev):
    out = git('rev-parse', '--verify', '-q', f'{rev}^{{commit}}')
    return out.strip() if out else None


def scan_diff(old, new, cached=False, strict=False):
    """Compare each changed file between old and new (new=None means the index).

    strict=True flags any signature in a changed file, even one already present in old. Used when code
    leaves this machine (pre-commit, pre-push), so previously committed malware cannot be re-committed or pushed."""
    if cached:
        names = git('diff', '--cached', '--name-only', '--diff-filter=ACMR', '-z', old or EMPTY_TREE)
    else:
        names = git('diff', '--name-only', '--diff-filter=ACMR', '-z', old or EMPTY_TREE, new)
    if names is None:
        raise SystemExit('npm-guard: could not diff the revisions; refusing to assume they are clean')
    found = []
    for path in filter(None, names.split('\0')):
        if not is_source(path):
            continue
        after = hits(blob(None if cached else new, path))
        if not after:
            continue
        before = {} if strict else (hits(blob(old, path)) if old else {})
        grew = {k: v for k, v in after.items() if v > before.get(k, 0)}
        if grew:
            found.append((path, grew))
    return found


def walk_files(root):
    """No git (Docker image, release tarball): every file except dependencies and build caches."""
    for dirpath, dirnames, filenames in os.walk(root):
        dirnames[:] = [d for d in dirnames if d not in ('node_modules', '.git', '.next')]
        for name in filenames:
            yield os.path.relpath(os.path.join(dirpath, name), root).replace(os.sep, '/')


def scan_worktree():
    root = git('rev-parse', '--show-toplevel')
    if root is None:
        root, paths = os.getcwd(), walk_files(os.getcwd())
    else:
        root = root.strip()
        paths = filter(None, (git('ls-files', '-z', '--cached', '--others', '--exclude-standard') or '').split('\0'))
    found = []
    for path in paths:
        full = os.path.join(root, path)
        if not is_source(path) or os.path.islink(full) or not os.path.isfile(full):
            continue
        try:
            with open(full, 'rb') as f:
                data = f.read(MAX_BYTES + 1)
        except OSError:
            continue
        found_here = hits(data)
        if found_here:
            found.append((path, found_here))
    return found


def report(found, context, blocking):
    allow = {p.strip() for p in os.environ.get('NPM_GUARD_SCAN_ALLOW', '').split(',') if p.strip()}
    found = [(p, h) for p, h in found if p not in allow]
    if not found:
        return 0
    bar = '=' * 72
    print(f'\n{bar}\nnpm-guard: MALWARE SIGNATURE FOUND {context}\n{bar}', file=sys.stderr)
    for path, h in found:
        print(f'  {path}: ' + ', '.join(f'{k} x{v}' for k, v in h.items()), file=sys.stderr)
    if blocking:
        print('\nBlocked. Do NOT run npm/node/next in this tree. Tell security and the pusher; '
              'their machine and tokens are likely compromised.', file=sys.stderr)
    else:
        print('\nThis code is ALREADY in your working tree. Do NOT run npm run dev/build, next, vite '
              'or any tool that loads these files.\n'
              '  1. Undo the update:   git reset --hard ORIG_HEAD   (after a pull/merge)  or  git checkout -\n'
              '  2. Tell security and the person who pushed it.\n'
              '  3. If you already ran the project since pulling: rotate tokens/SSH keys/.env secrets.',
              file=sys.stderr)
    print(bar, file=sys.stderr)
    return 1


def main():
    for stream in (sys.stdout, sys.stderr):  # Windows consoles default to a legacy codepage; never crash on a filename
        if hasattr(stream, 'reconfigure'):
            stream.reconfigure(encoding='utf-8', errors='replace')
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    g = ap.add_mutually_exclusive_group(required=True)
    g.add_argument('--range', nargs=2, metavar=('OLD', 'NEW'))
    g.add_argument('--ref', metavar='REF')
    g.add_argument('--staged', action='store_true')
    g.add_argument('--worktree', action='store_true')
    ap.add_argument('--strict', action='store_true', help='with --range: flag any signature in changed files, not only new ones (pre-push)')
    ap.add_argument('--warn-only', action='store_true', help='print findings but always exit 0 (post-* hooks)')
    args = ap.parse_args()

    blocking = not args.warn_only
    if args.worktree:
        found, context = scan_worktree(), 'in your working tree'
    elif args.staged:
        found, context = scan_diff(resolve('HEAD'), None, cached=True, strict=True), 'in the changes you are committing'
    elif args.ref:
        new = resolve(args.ref)
        if new is None:
            raise SystemExit(f'npm-guard: cannot resolve {args.ref}')
        base = (git('merge-base', 'HEAD', new) or '').strip() or None
        found, context = scan_diff(base, new), f'in incoming {args.ref}'
    else:
        old, new = args.range
        zero = set('0')
        old = None if set(old) <= zero else (resolve(old) or None)
        new = resolve(new)
        if new is None:
            return 0
        found, context = scan_diff(old, new, strict=args.strict), 'in the revisions just brought in'
    code = report(found, context, blocking)
    return 0 if args.warn_only else code


if __name__ == '__main__':
    sys.exit(main())
