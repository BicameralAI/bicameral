"""Behavior tests for tools/npm-guard.

Run from the repository root:
    python3 -m unittest discover -s tools/npm-guard/tests -v

Needs git, node and npm. Tests that ask the public npm registry skip themselves when it is unreachable.
Every git test works in its own temporary repository with an empty global git config, so nothing here
touches your clone or your settings. The malware signature is assembled at run time, so this file
never contains it.
"""

from __future__ import annotations

import contextlib
import importlib.util
import io
import json
import os
import shutil
import subprocess
import sys
import tempfile
import unittest
import urllib.error
import urllib.parse
import urllib.request
from datetime import datetime, timedelta, timezone
from pathlib import Path
from unittest import mock

GUARD = Path(__file__).resolve().parents[1]
REPO = GUARD.parents[1]
DASHBOARD = REPO / 'apps' / 'bot' / 'dashboard'
GATE = GUARD / 'gate.cjs'
NPM = shutil.which('npm')
NODE = shutil.which('node')
SH = shutil.which('sh')
# A harmless file that matches the scanner's main signature (the global `o` assignment).
INFECTED = 'module.exports = {};\n' + 'glob' + "al.o='npm-guard-test';\n"
FOUND = 'MALWARE SIGNATURE FOUND'


def registry_reachable() -> bool:
    try:
        with urllib.request.urlopen('https://registry.npmjs.org/-/ping', timeout=5):
            return True
    except Exception:
        return False


ONLINE = registry_reachable()


def run(args, cwd, env=None, timeout=120):
    return subprocess.run(args, cwd=cwd, env=env, capture_output=True, text=True, timeout=timeout)


def load(name):
    spec = importlib.util.spec_from_file_location(name, GUARD / f'{name}.py')
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def isotime(days_ago):
    return (datetime.now(timezone.utc) - timedelta(days=days_ago)).isoformat().replace('+00:00', 'Z')


class FakeRegistry:
    """Stands in for registry.npmjs.org and api.npmjs.org inside one test (the external dependency)."""

    def __init__(self):
        self.packages = {}
        self.downloads = {}
        self.downloads_down = False

    def add(self, name, versions, latest, created_days_ago=400, weekly=50000):
        """versions maps each version to its age in days."""
        self.packages[name] = {
            'dist-tags': {'latest': latest},
            'versions': {v: {} for v in versions},
            'time': {'created': isotime(created_days_ago), **{v: isotime(d) for v, d in versions.items()}},
        }
        self.downloads[name] = weekly

    def fetch_json(self, url):
        if url.startswith('https://api.npmjs.org/downloads/'):
            if self.downloads_down:
                raise urllib.error.HTTPError(url, 503, 'unavailable', None, io.BytesIO())
            return {'downloads': self.downloads.get(urllib.parse.unquote(url.rsplit('/', 1)[1]), 0)}
        name = urllib.parse.unquote(url.split('registry.npmjs.org/', 1)[1])
        if name not in self.packages:
            raise urllib.error.HTTPError(url, 404, 'not found', None, io.BytesIO())
        return self.packages[name]


@unittest.skipUnless(NPM, 'npm is not installed')
class IgnoreScriptsNegativeControl(unittest.TestCase):
    """ignore-scripts=true also skips npm pre/post scripts, so the guard must run inline."""

    def setUp(self):
        self.tree = Path(tempfile.mkdtemp(prefix='npm-guard-control-'))
        self.addCleanup(shutil.rmtree, self.tree, True)
        (self.tree / 'package.json').write_text(json.dumps({
            'name': 'control', 'version': '1.0.0', 'private': True,
            'scripts': {
                'preprobe': 'exit 17',
                'probe': 'node -e ""',
                'guarded': f'node "{GATE}" --gate && node -e ""',
            },
        }))
        self.env = {**os.environ, 'NPM_GUARD_ROOT': str(self.tree)}

    def npm_run(self, ignore_scripts, script):
        return run([NPM, f'--ignore-scripts={ignore_scripts}', 'run', script], self.tree, self.env)

    def test_pre_script_runs_only_when_scripts_are_enabled(self):
        self.assertEqual(self.npm_run('false', 'probe').returncode, 17)
        self.assertEqual(self.npm_run('true', 'probe').returncode, 0)

    def test_inline_guard_refuses_an_infected_tree_with_scripts_disabled(self):
        self.assertEqual(self.npm_run('true', 'guarded').returncode, 0)
        (self.tree / 'vite.config.js').write_text(INFECTED)
        result = self.npm_run('true', 'guarded')
        self.assertNotEqual(result.returncode, 0)
        self.assertIn(FOUND, result.stderr + result.stdout)


@unittest.skipUnless(NPM, 'npm is not installed')
class DashboardScripts(unittest.TestCase):
    """Every dashboard script that runs project code refuses to start while the tree is infected."""

    def test_every_script_refuses_an_infected_tree(self):
        scripts = json.loads((DASHBOARD / 'package.json').read_text())['scripts']
        project_scripts = [name for name in scripts if not name.startswith('security:')]
        self.assertTrue(project_scripts)
        planted = DASHBOARD / 'npm-guard-test-infected.config.js'
        planted.write_text(INFECTED)
        self.addCleanup(planted.unlink)
        for name in project_scripts:
            with self.subTest(script=name):
                try:
                    result = run([NPM, '--ignore-scripts=true', 'run', name], DASHBOARD, timeout=60)
                except subprocess.TimeoutExpired:
                    self.fail(f'"{name}" started instead of refusing')
                self.assertNotEqual(result.returncode, 0)
                self.assertIn(FOUND, result.stderr + result.stdout)


class Verify(unittest.TestCase):
    """gate.cjs --verify fails when the guard is removed or the dashboard .npmrc is weakened."""

    REQUIRED = ['tools/npm-guard/hooks/pre-commit', 'tools/npm-guard/hooks/pre-push',
                'tools/npm-guard/hooks/pre-merge-commit', 'tools/npm-guard/hooks/post-merge',
                'tools/npm-guard/hooks/post-rewrite', 'tools/npm-guard/hooks/post-checkout',
                'tools/npm-guard/scan_incoming.py', 'tools/npm-guard/check_dependency_changes.py',
                'tools/npm-guard/registry_check.py',
                'apps/bot/dashboard/.npmrc']

    def setUp(self):
        self.tree = Path(tempfile.mkdtemp(prefix='npm-guard-verify-'))
        self.addCleanup(shutil.rmtree, self.tree, True)
        for rel in self.REQUIRED:
            (self.tree / rel).parent.mkdir(parents=True, exist_ok=True)
            shutil.copy(REPO / rel, self.tree / rel)

    def verify(self):
        return run(['node', str(GATE), '--verify'], self.tree, {**os.environ, 'NPM_GUARD_ROOT': str(self.tree)})

    def test_intact_guard_passes(self):
        self.assertEqual(self.verify().returncode, 0, self.verify().stderr)

    def test_weakened_npmrc_fails(self):
        npmrc = self.tree / 'apps/bot/dashboard/.npmrc'
        npmrc.write_text(npmrc.read_text().replace('ignore-scripts=true', 'ignore-scripts=false'))
        result = self.verify()
        self.assertEqual(result.returncode, 1)
        self.assertIn('must set ignore-scripts=true', result.stderr)

    def test_a_later_line_that_weakens_npmrc_fails(self):
        npmrc = self.tree / 'apps/bot/dashboard/.npmrc'
        original = npmrc.read_text()
        for line in ('ignore-scripts=false', 'min-release-age=0', 'registry=https://npm.example.invalid/',
                     '@bicameral:registry=https://npm.example.invalid/'):
            with self.subTest(line=line):
                npmrc.write_text(original + line + '\n')
                self.assertEqual(self.verify().returncode, 1)

    def test_removed_hook_fails(self):
        (self.tree / 'tools/npm-guard/hooks/pre-push').unlink()
        result = self.verify()
        self.assertEqual(result.returncode, 1)
        self.assertIn('MISSING tools/npm-guard/hooks/pre-push', result.stderr)


class GitHooks(unittest.TestCase):
    """The git hooks in a fresh clone with npm run security:setup applied."""

    def setUp(self):
        self.tmp = Path(tempfile.mkdtemp(prefix='npm-guard-git-'))
        self.addCleanup(shutil.rmtree, self.tmp, True)
        empty = self.tmp / 'gitconfig'
        empty.write_text('')
        self.env = {**os.environ, 'GIT_CONFIG_GLOBAL': str(empty), 'GIT_CONFIG_NOSYSTEM': '1',
                    'GIT_AUTHOR_NAME': 'test', 'GIT_AUTHOR_EMAIL': 'test@example.invalid',
                    'GIT_COMMITTER_NAME': 'test', 'GIT_COMMITTER_EMAIL': 'test@example.invalid'}
        for key in ('NPM_GUARD_ALLOW', 'NPM_GUARD_SCAN_ALLOW'):
            self.env.pop(key, None)
        self.remote = self.tmp / 'remote.git'
        self.git(self.tmp, 'init', '-q', '--bare', '-b', 'main', str(self.remote))
        self.work = self.tmp / 'work'
        self.work.mkdir()
        self.git(self.work, 'init', '-q', '-b', 'main')
        shutil.copytree(GUARD, self.work / 'tools' / 'npm-guard',
                        ignore=shutil.ignore_patterns('tests', '__pycache__'))
        self.write('app/package.json', json.dumps({'name': 'app', 'version': '1.0.0', 'dependencies': {}}))
        self.write('app/package-lock.json', json.dumps(
            {'name': 'app', 'lockfileVersion': 3, 'packages': {'': {'name': 'app', 'version': '1.0.0'}}}))
        setup = run(['sh', 'tools/npm-guard/setup.sh'], self.work, self.env)
        self.assertEqual(setup.returncode, 0, setup.stderr)
        self.git(self.work, 'add', '-A')
        self.git(self.work, 'commit', '-q', '-m', 'base')
        self.git(self.work, 'remote', 'add', 'origin', str(self.remote))
        self.git(self.work, 'push', '-q', '-u', 'origin', 'main')

    def git(self, cwd, *args, check=True):
        result = run(['git', *args], cwd, self.env)
        if check and result.returncode != 0:
            self.fail(f'git {" ".join(args)} failed: {result.stderr}')
        return result

    def write(self, rel, text, root=None):
        path = (root or self.work) / rel
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(text)

    def commit(self, message, *extra, env=None):
        self.git(self.work, 'add', '-A')
        return run(['git', 'commit', '-q', *extra, '-m', message], self.work, env or self.env)

    def add_dependency(self, name, spec, lockfile=True):
        self.write('app/package.json', json.dumps(
            {'name': 'app', 'version': '1.0.0', 'dependencies': {name: spec}}))
        if lockfile:
            self.write('app/package-lock.json', json.dumps({'name': 'app', 'lockfileVersion': 3, 'packages': {
                '': {'name': 'app', 'version': '1.0.0', 'dependencies': {name: spec}}}}))

    def test_commit_with_malware_signature_is_blocked(self):
        self.write('app/vite.config.js', INFECTED)
        result = self.commit('infected')
        self.assertNotEqual(result.returncode, 0)
        self.assertIn(FOUND, result.stderr)
        self.assertEqual(self.git(self.work, 'log', '--format=%s').stdout.split(), ['base'])

    def test_git_dependency_is_blocked(self):
        self.add_dependency('left-pad', 'github:someone/left-pad')
        result = self.commit('git dep')
        self.assertNotEqual(result.returncode, 0)
        self.assertIn('non-registry spec', result.stderr)

    def test_dependency_change_without_lockfile_is_blocked(self):
        self.add_dependency('react', '^18.3.1', lockfile=False)
        result = self.commit('no lockfile', env={**self.env, 'NPM_GUARD_ALLOW': 'react'})
        self.assertNotEqual(result.returncode, 0)
        self.assertIn('no lockfile in that directory changed', result.stderr)

    @unittest.skipUnless(ONLINE, 'npm registry is unreachable')
    def test_package_missing_from_registry_is_blocked_unless_allowed(self):
        name = 'npm-guard-test-package-that-does-not-exist-7c1f'
        self.add_dependency(name, '^1.0.0')
        result = self.commit('missing package')
        self.assertNotEqual(result.returncode, 0)
        self.assertIn('does not exist on the npm registry', result.stderr)
        self.write('tools/npm-guard/allow.txt', f'{name}  # reviewed in the test\n')
        self.assertEqual(self.commit('allowed').returncode, 0)

    def test_commit_of_a_file_without_a_suffix_is_blocked(self):
        self.write('scripts/deploy', '#!/usr/bin/env node\n' + INFECTED)
        result = self.commit('infected script')
        self.assertNotEqual(result.returncode, 0)
        self.assertIn('scripts/deploy', result.stderr)

    def test_commit_made_with_no_verify_is_blocked_at_push(self):
        self.write('app/vite.config.js', INFECTED)
        self.assertEqual(self.commit('infected', '--no-verify').returncode, 0)
        result = self.git(self.work, 'push', 'origin', 'main', check=False)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn(FOUND, result.stderr)
        remote_log = self.git(self.remote, 'log', '--format=%s', 'main').stdout.split()
        self.assertEqual(remote_log, ['base'])

    def test_checkout_does_not_run_hooks_from_the_checked_out_branch(self):
        marker = self.tmp / 'hostile-hook-ran'
        self.git(self.work, 'checkout', '-q', '-b', 'hostile')
        for hook in ('post-checkout', 'post-merge'):
            self.write(f'tools/npm-guard/hooks/{hook}', f'#!/bin/sh\ntouch "{marker}"\n')
        self.write('tools/npm-guard/scan_incoming.py', f'open({str(marker)!r}, "w").close()\n')
        self.assertEqual(self.commit('hostile hooks', '--no-verify').returncode, 0)
        self.git(self.work, 'checkout', '-q', 'main')
        self.git(self.work, 'checkout', '-q', 'hostile')
        self.git(self.work, 'checkout', '-q', 'main')
        self.git(self.work, 'merge', '-q', '--ff-only', 'hostile', check=False)
        self.assertFalse(marker.exists(), 'a hook from the checked-out branch ran')

    def test_safe_pull_refuses_infected_commits_and_leaves_files_untouched(self):
        other = self.tmp / 'other'
        self.git(self.tmp, 'clone', '-q', str(self.remote), str(other))
        self.write('app/vite.config.js', INFECTED, root=other)
        self.git(other, 'add', '-A')
        self.git(other, 'commit', '-q', '-m', 'infected')
        self.git(other, 'push', '-q', 'origin', 'main')  # no hooks in this clone
        result = self.git(self.work, 'safe-pull', check=False)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn('NOT pulling', result.stderr)
        self.assertFalse((self.work / 'app' / 'vite.config.js').exists())


class DependencyCheck(unittest.TestCase):
    """check_dependency_changes.py against a fake registry: what CI and the hooks block."""

    def setUp(self):
        self.tmp = Path(tempfile.mkdtemp(prefix='npm-guard-deps-'))
        self.addCleanup(shutil.rmtree, self.tmp, True)
        empty = self.tmp / 'gitconfig'
        empty.write_text('')
        env = {'GIT_CONFIG_GLOBAL': str(empty), 'GIT_CONFIG_NOSYSTEM': '1',
               'GIT_AUTHOR_NAME': 't', 'GIT_AUTHOR_EMAIL': 't@example.invalid',
               'GIT_COMMITTER_NAME': 't', 'GIT_COMMITTER_EMAIL': 't@example.invalid'}
        patcher = mock.patch.dict(os.environ, env)
        patcher.start()
        self.addCleanup(patcher.stop)
        os.environ.pop('NPM_GUARD_ALLOW', None)
        self.repo = self.tmp / 'repo'
        self.repo.mkdir()
        self.git('init', '-q', '-b', 'main')
        self.registry = FakeRegistry()
        self.registry.add('vite', {'6.4.0': 300, '6.4.4': 1}, latest='6.4.4')
        self.write_app({'vite': '^6.4.0'}, {'vite': ('vite', '6.4.0', 'sha512-old')})
        self.git('add', '-A')
        self.git('commit', '-q', '-m', 'base')

    def git(self, *args):
        result = run(['git', *args], self.repo)
        self.assertEqual(result.returncode, 0, result.stderr)
        return result

    def write_app(self, deps, lock):
        """lock maps each node_modules key to (registry name, version, integrity)."""
        app = self.repo / 'app'
        app.mkdir(exist_ok=True)
        (app / 'package.json').write_text(json.dumps({'name': 'app', 'dependencies': deps}))
        packages = {'': {'name': 'app', 'dependencies': deps}}
        for key, (name, version, integrity) in lock.items():
            packages[f'node_modules/{key}'] = {
                **({'name': name} if name != key else {}), 'version': version, 'integrity': integrity,
                'resolved': f'https://registry.npmjs.org/{name}/-/{name.split("/")[-1]}-{version}.tgz'}
        (app / 'package-lock.json').write_text(json.dumps({'name': 'app', 'lockfileVersion': 3, 'packages': packages}))

    def check(self):
        self.git('add', '-A')
        guard = load('registry_check')
        checker = load('check_dependency_changes')
        stderr = io.StringIO()
        cwd = os.getcwd()
        os.chdir(self.repo)
        try:
            with mock.patch.object(guard, 'fetch_json', self.registry.fetch_json), \
                    mock.patch.object(checker, 'load_registry_check', lambda: guard.check_package), \
                    mock.patch.object(sys, 'argv', ['check', '--staged']), contextlib.redirect_stderr(stderr):
                code = checker.main()
        finally:
            os.chdir(cwd)
        return code, stderr.getvalue()

    def test_pinned_older_version_passes_while_latest_is_new(self):
        self.registry.add('zod', {'3.23.8': 100, '4.0.0': 2}, latest='4.0.0')
        self.write_app({'vite': '^6.4.0', 'zod': '3.23.8'}, {
            'vite': ('vite', '6.4.0', 'sha512-old'), 'zod': ('zod', '3.23.8', 'sha512-zod')})
        code, out = self.check()
        self.assertEqual(code, 0, out)

    def test_lockfile_bump_to_a_version_under_the_cooldown_is_blocked(self):
        self.write_app({'vite': '^6.4.0'}, {'vite': ('vite', '6.4.4', 'sha512-new')})
        code, out = self.check()
        self.assertEqual(code, 1)
        self.assertIn('6.4.4 published 1d ago', out)

    def test_reviewed_allow_entry_accepts_an_urgent_fix(self):
        self.write_app({'vite': '^6.4.0'}, {'vite': ('vite', '6.4.4', 'sha512-new')})
        allow = self.repo / 'tools' / 'npm-guard' / 'allow.txt'
        allow.parent.mkdir(parents=True)
        allow.write_text('vite  # security fix, advisory linked in the pull request\n')
        code, out = self.check()
        self.assertEqual(code, 0, out)

    def test_alias_swap_on_an_existing_name_is_vetted(self):
        self.write_app({'vite': 'npm:vite-evil@6.4.4'}, {'vite': ('vite-evil', '6.4.4', 'sha512-evil')})
        code, out = self.check()
        self.assertEqual(code, 1)
        self.assertIn('"vite-evil" does not exist', out)

    def test_integrity_change_without_a_new_version_is_blocked(self):
        self.write_app({'vite': '^6.4.0'}, {'vite': ('vite', '6.4.0', 'sha512-swapped')})
        code, out = self.check()
        self.assertEqual(code, 1)
        self.assertIn('integrity changed', out)

    def test_download_stats_outage_only_warns(self):
        self.registry.add('zod', {'3.23.8': 100}, latest='3.23.8')
        self.registry.downloads_down = True
        self.write_app({'vite': '^6.4.0', 'zod': '3.23.8'}, {
            'vite': ('vite', '6.4.0', 'sha512-old'), 'zod': ('zod', '3.23.8', 'sha512-zod')})
        code, out = self.check()
        self.assertEqual(code, 0, out)
        self.assertIn('warning:', out)


class GateFailsClosed(unittest.TestCase):
    """gate.cjs --gate stops the npm script when the scan cannot run, so the build after `&&` never starts."""

    def setUp(self):
        self.tmp = Path(tempfile.mkdtemp(prefix='npm-guard-gate-'))
        self.addCleanup(shutil.rmtree, self.tmp, True)
        self.marker = self.tmp / 'build-ran'

    def guarded_build(self, gate, path):
        # The shape of every dashboard script: `node gate.cjs --gate && <build>`, run by sh as npm does.
        command = f'"{NODE}" "{gate}" --gate && : > "{self.marker}"'
        env = {**os.environ, 'PATH': path, 'NPM_GUARD_ROOT': str(self.tmp)}
        return run([SH, '-c', command], self.tmp, env)

    @unittest.skipUnless(NODE and SH, 'needs node and sh')
    def test_build_does_not_run_without_python(self):
        empty_path = self.tmp / 'bin'
        empty_path.mkdir()
        self.assertEqual(self.guarded_build(GATE, os.environ['PATH']).returncode, 0)
        self.marker.unlink()
        result = self.guarded_build(GATE, str(empty_path))
        self.assertEqual(result.returncode, 1)
        self.assertIn('Python 3.6+ not found', result.stderr)
        self.assertFalse(self.marker.exists(), 'the build ran without a malware scan')

    @unittest.skipUnless(NODE and SH, 'needs node and sh')
    def test_build_does_not_run_without_the_scanner(self):
        lone = self.tmp / 'guard'
        lone.mkdir()
        shutil.copy(GATE, lone / 'gate.cjs')
        result = self.guarded_build(lone / 'gate.cjs', os.environ['PATH'])
        self.assertEqual(result.returncode, 1)
        self.assertIn('scan_incoming.py is missing', result.stderr)
        self.assertFalse(self.marker.exists(), 'the build ran without a malware scan')


class Scanner(unittest.TestCase):
    """scan_incoming.py --worktree in a git repository, the way the gate runs it."""

    def setUp(self):
        self.tree = Path(tempfile.mkdtemp(prefix='npm-guard-scan-'))
        self.addCleanup(shutil.rmtree, self.tree, True)
        run(['git', 'init', '-q'], self.tree)

    def write(self, rel, text):
        path = self.tree / rel
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(text)
        return path

    def scan(self):
        return run(['python3', str(GUARD / 'scan_incoming.py'), '--worktree'], self.tree)

    def test_signature_is_found_in_every_file_type(self):
        for rel in ('setup.py', 'index.html', 'styles.css', '.github/ci.yml', 'install.ps1', 'bin/run',
                    'assets/logo.png'):
            with self.subTest(file=rel):
                planted = self.write(rel, INFECTED)
                result = self.scan()
                planted.unlink()
                self.assertEqual(result.returncode, 1)
                self.assertIn(rel, result.stderr)

    def test_a_file_over_the_size_limit_is_reported(self):
        self.write('assets/blob.dat', '//' + ' ' * (6 * 1024 * 1024) + '\n' + INFECTED)
        result = self.scan()
        self.assertEqual(result.returncode, 1)
        self.assertIn('over 5 MiB', result.stderr)

    @unittest.skipUnless(hasattr(os, 'symlink') and os.name != 'nt', 'needs POSIX symbolic links')
    def test_a_symbolic_link_to_an_infected_file_outside_the_tree_is_found(self):
        outside = Path(tempfile.mkdtemp(prefix='npm-guard-outside-'))
        self.addCleanup(shutil.rmtree, outside, True)
        (outside / 'payload.js').write_text(INFECTED)
        os.symlink(outside / 'payload.js', self.tree / 'vite.config.js')
        result = self.scan()
        self.assertEqual(result.returncode, 1)
        self.assertIn('vite.config.js', result.stderr)

    @unittest.skipUnless(hasattr(os, 'symlink') and os.name != 'nt', 'needs POSIX symbolic links')
    def test_a_symbolic_link_to_a_directory_is_reported(self):
        outside = Path(tempfile.mkdtemp(prefix='npm-guard-outside-'))
        self.addCleanup(shutil.rmtree, outside, True)
        (outside / 'index.js').write_text('module.exports = {};\n')
        os.symlink(outside, self.tree / 'lib')
        result = self.scan()
        self.assertEqual(result.returncode, 1)
        self.assertIn('symbolic link to a directory', result.stderr)


if __name__ == '__main__':
    unittest.main()
