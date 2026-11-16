#!/usr/bin/env python3
"""Local pre-commit / pre-push check for risky npm dependency changes.

Works with any tool or editor because it runs from git hooks. Compares the
package.json / lockfile files in a change against a base revision:

  --staged                 compare HEAD with the index (pre-commit)
  --base REV --head REV    compare two revisions (pre-push)

Fails (exit 1) when a change:
  * adds a git / URL / GitHub-shorthand dependency
  * adds or changes a preinstall / install / postinstall script in the project itself
  * changes dependencies without changing a lockfile in the same directory
  * adds a package that does not exist on the registry, or is new / barely used
  * adds a package-lock.json "resolved" URL outside the allowed registries

Warns (exit 0) when the registry is unreachable or a new dependency declares install scripts.

Acknowledge a reviewed package for one run with NPM_GUARD_ALLOW=name1,name2, or permanently by
adding it to tools/npm-guard/allow.txt (code owners review that file in the pull request).
Allow an internal registry with NPM_GUARD_REGISTRY_HOSTS=host1,host2.
Skip everything with `git commit/push --no-verify` (CI still re-checks).
"""
import argparse
import importlib.util
import json
import os
import re
import subprocess
import sys
from pathlib import PurePosixPath

EMPTY_TREE = '4b825dc642cb6eb9a060e54bf8d69288fbee4904'
DEP_SECTIONS = ('dependencies', 'devDependencies', 'optionalDependencies', 'peerDependencies')
INSTALL_SCRIPTS = ('preinstall', 'install', 'postinstall')
LOCKFILES = ('package-lock.json', 'pnpm-lock.yaml', 'yarn.lock', 'bun.lock', 'bun.lockb')
REMOTE_SPEC_RE = re.compile(r'^(git\+|git:|github:|gitlab:|bitbucket:|https?:|[\w.-]+/[\w.-]+(#.*)?$)')
LOCAL_SPEC_RE = re.compile(r'^(workspace:|file:|link:|\.|/|~/)')


def git(*args):
    result = subprocess.run(['git', *args], capture_output=True, text=True)
    return result.stdout if result.returncode == 0 else None


def show(rev, path):
    """File contents at rev (None = the index), or None if absent."""
    return git('show', f'{rev}:{path}' if rev else f':{path}')


def load_json(rev, path):
    text = show(rev, path)
    try:
        return json.loads(text) if text else {}
    except ValueError:
        return {}


def changed_files(base, head):
    args = ['diff', '--name-only', '--diff-filter=ACMR']
    args += ['--cached', base] if head is None else [base, head]
    return [p for p in (git(*args) or '').splitlines() if 'node_modules/' not in p]


def real_name(name, spec):
    """npm:alias@ver specs resolve to a different registry name."""
    if spec.startswith('npm:'):
        target = spec[4:]
        at = target.rfind('@')
        return target[:at] if at > 0 else target
    return name


def load_registry_check():
    """Reuse the Claude hook's registry vetting so the logic lives in one place."""
    path = os.path.join(os.path.dirname(os.path.abspath(__file__)), 'guard_npm_install.py')
    try:
        spec = importlib.util.spec_from_file_location('guard_npm_install', path)
        module = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(module)
        return module.check_package
    except Exception:
        return None


def allow_file_names():
    """Package names in tools/npm-guard/allow.txt of the repository being checked.

    The file is read from the checked tree, not from the guard's own copy, so a pull request that
    allows a package shows the entry in its diff, where code owners review it."""
    top = (git('rev-parse', '--show-toplevel') or '').strip()
    try:
        with open(os.path.join(top, 'tools', 'npm-guard', 'allow.txt'), encoding='utf-8') as handle:
            return {line.split('#', 1)[0].strip() for line in handle} - {''}
    except OSError:
        return set()


def check_manifest(path, base, head, allow, errors, to_vet):
    old, new = load_json(base, path), load_json(head, path)
    for section in DEP_SECTIONS:
        old_deps, new_deps = old.get(section, {}), new.get(section, {})
        for name, spec in new_deps.items():
            if old_deps.get(name) == spec:
                continue
            if LOCAL_SPEC_RE.match(spec):
                continue
            if REMOTE_SPEC_RE.match(spec):
                errors.append(f'{path}: "{name}" uses a non-registry spec "{spec}" ({section}).')
                continue
            if name not in old_deps:
                target = real_name(name, spec)
                if target not in allow:
                    to_vet.add((path, target))
    for script in INSTALL_SCRIPTS:
        if new.get('scripts', {}).get(script) and new['scripts'].get(script) != old.get('scripts', {}).get(script):
            errors.append(f'{path}: adds/changes a "{script}" script, which runs on every install.')
    return old, new


def deps_changed(old, new):
    return any(old.get(s, {}) != new.get(s, {}) for s in DEP_SECTIONS)


def check_lockfile_present(path, base, head, files, errors):
    folder = PurePosixPath(path).parent
    candidates = [str(folder / name) for name in LOCKFILES]
    if not any(c in files for c in candidates):
        errors.append(f'{path}: dependencies changed but no lockfile in that directory changed. '
                      'Run your package manager install (with --ignore-scripts) and commit the lockfile.')


def check_package_lock(path, base, head, hosts, errors, warnings):
    old = load_json(base, path).get('packages', {})
    new = load_json(head, path).get('packages', {})
    old_resolved = {e.get('resolved') for e in old.values()}
    for key, entry in new.items():
        resolved = entry.get('resolved')
        if resolved and resolved not in old_resolved:
            host = re.sub(r'^\w+://([^/]+).*', r'\1', resolved)
            if host not in hosts and not resolved.startswith(('file:', '.')):
                errors.append(f'{path}: "{key or "."}" resolves from unexpected host {host}.')
        if entry.get('hasInstallScript') and not old.get(key, {}).get('hasInstallScript'):
            warnings.append(f'{path}: {key} declares install scripts (keep ignore-scripts=true).')


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument('--staged', action='store_true')
    parser.add_argument('--base')
    parser.add_argument('--head')
    args = parser.parse_args()
    if args.staged:
        base = 'HEAD' if git('rev-parse', '--verify', 'HEAD') else EMPTY_TREE
        head = None
    elif args.base and args.head:
        base, head = args.base, args.head
    else:
        parser.error('use --staged, or both --base and --head')

    allow = {n for n in os.environ.get('NPM_GUARD_ALLOW', '').split(',') if n} | allow_file_names()
    hosts = {'registry.npmjs.org', *[h for h in os.environ.get('NPM_GUARD_REGISTRY_HOSTS', '').split(',') if h]}
    files = changed_files(base, head)
    errors, warnings, to_vet = [], [], set()

    for path in files:
        name = PurePosixPath(path).name
        if name == 'package.json':
            old, new = check_manifest(path, base, head, allow, errors, to_vet)
            if deps_changed(old, new):
                check_lockfile_present(path, base, head, files, errors)
        elif name == 'package-lock.json':
            check_package_lock(path, base, head, hosts, errors, warnings)

    if to_vet:
        check_package = load_registry_check()
        if check_package is None:
            warnings.append('Registry checks skipped (tools/npm-guard/guard_npm_install.py not found).')
        else:
            for path, name in sorted(to_vet):
                verdict = check_package(name)
                if not verdict:
                    continue
                decision, reason = verdict
                if reason.startswith('Could not verify'):
                    warnings.append(f'{path}: {reason}')
                else:
                    errors.append(f'{path}: {reason} (review it, then NPM_GUARD_ALLOW={name} to proceed)')

    for message in warnings:
        print(f'warning: {message}', file=sys.stderr)
    for message in errors:
        print(f'BLOCKED: {message}', file=sys.stderr)
    if errors:
        print('\nDependency check failed. Fix the above, or bypass with --no-verify (CI will re-check).',
              file=sys.stderr)
        return 1
    return 0


if __name__ == '__main__':
    sys.exit(main())
