"""Behavior tests for tools/npm-guard.

Run from the repository root:
    python3 -m unittest discover -s tools/npm-guard/tests -v

Needs git, node and npm. Tests that ask the public npm registry skip themselves when it is unreachable.
Every git test works in its own temporary repository with an empty global git config, so nothing here
touches your clone or your settings. The malware signature is assembled at run time, so this file
never contains it.
"""

from __future__ import annotations

import json
import os
import shutil
import subprocess
import tempfile
import unittest
import urllib.request
from pathlib import Path

GUARD = Path(__file__).resolve().parents[1]
REPO = GUARD.parents[1]
DASHBOARD = REPO / 'apps' / 'bot' / 'dashboard'
GATE = GUARD / 'gate.cjs'
NPM = shutil.which('npm')
# A harmless file that matches the scanner's `global.o=` signature.
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


class ClaudeHook(unittest.TestCase):
    """The Claude Code PreToolUse hook (guard_npm_install.py)."""

    def decide(self, command, cwd):
        event = json.dumps({'tool_name': 'Bash', 'tool_input': {'command': command}, 'cwd': str(cwd)})
        result = subprocess.run(['python3', str(GUARD / 'guard_npm_install.py')], input=event, cwd=REPO,
                                capture_output=True, text=True, timeout=60,
                                env={**os.environ, 'CLAUDE_PROJECT_DIR': str(REPO)})
        out = result.stdout.strip()
        return json.loads(out)['hookSpecificOutput'] if out else None

    def test_install_uses_the_npmrc_of_the_project_it_runs_in(self):
        self.assertIsNone(self.decide('npm install', DASHBOARD / 'src'))
        bare = Path(tempfile.mkdtemp(prefix='npm-guard-hook-'))
        self.addCleanup(shutil.rmtree, bare, True)
        (bare / 'package.json').write_text('{}')
        self.assertEqual(self.decide('npm install', bare)['permissionDecision'], 'ask')

    def test_node_commands_are_denied_while_the_tree_is_infected(self):
        self.assertIsNone(self.decide('npm run dev', DASHBOARD))
        planted = DASHBOARD / 'npm-guard-test-infected.config.js'
        planted.write_text(INFECTED)
        self.addCleanup(planted.unlink)
        self.assertEqual(self.decide('npm run dev', DASHBOARD)['permissionDecision'], 'deny')

    @unittest.skipUnless(ONLINE, 'npm registry is unreachable')
    def test_package_missing_from_registry_is_denied(self):
        decision = self.decide('npm install npm-guard-test-package-that-does-not-exist-7c1f', DASHBOARD)
        self.assertEqual(decision['permissionDecision'], 'deny')


class Verify(unittest.TestCase):
    """gate.cjs --verify fails when the guard is removed or the dashboard .npmrc is weakened."""

    REQUIRED = ['tools/npm-guard/hooks/pre-commit', 'tools/npm-guard/hooks/pre-push',
                'tools/npm-guard/hooks/pre-merge-commit', 'tools/npm-guard/hooks/post-merge',
                'tools/npm-guard/hooks/post-rewrite', 'tools/npm-guard/hooks/post-checkout',
                'tools/npm-guard/scan_incoming.py', 'tools/npm-guard/check_dependency_changes.py',
                'tools/npm-guard/guard_npm_install.py', '.claude/settings.json',
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
        self.assertIn('no longer sets ignore-scripts', result.stderr)

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
        self.git(self.work, 'config', 'core.hooksPath', 'tools/npm-guard/hooks')
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

    def test_commit_made_with_no_verify_is_blocked_at_push(self):
        self.write('app/vite.config.js', INFECTED)
        self.assertEqual(self.commit('infected', '--no-verify').returncode, 0)
        result = self.git(self.work, 'push', 'origin', 'main', check=False)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn(FOUND, result.stderr)
        remote_log = self.git(self.remote, 'log', '--format=%s', 'main').stdout.split()
        self.assertEqual(remote_log, ['base'])

    def test_safe_pull_refuses_infected_commits_and_leaves_files_untouched(self):
        other = self.tmp / 'other'
        self.git(self.tmp, 'clone', '-q', str(self.remote), str(other))
        self.write('app/vite.config.js', INFECTED, root=other)
        self.git(other, 'add', '-A')
        self.git(other, 'commit', '-q', '-m', 'infected')
        self.git(other, 'push', '-q', 'origin', 'main')  # no hooks in this clone
        result = run(['sh', 'tools/npm-guard/safe-pull.sh'], self.work, self.env)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn('NOT pulling', result.stderr)
        self.assertFalse((self.work / 'app' / 'vite.config.js').exists())


if __name__ == '__main__':
    unittest.main()
