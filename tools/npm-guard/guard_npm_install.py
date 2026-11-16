#!/usr/bin/env python3
"""Claude Code PreToolUse hook: vet package installs and package runs.

Reads the hook JSON on stdin. Checks every install (`npm|pnpm|yarn|bun install|add`, including npm's
install aliases and global flags such as --prefix) and every fetch-and-run (`npx`, `npm exec`,
`pnpm dlx`, `yarn dlx`, `bunx`) in the command. Then it emits the most severe decision once:
  deny  - any node/npm/npx/next/vite/... command while the working tree contains a malware signature
  deny  - a package does not exist on the registry, or the requested version is newer than the minimum release age
  ask   - a package is new or little used, a spec is not a registry package, lifecycle scripts are not
          disabled, or an unversioned or @latest install was switched to an older, settled version
  (no output) - nothing suspicious, defer to normal permission rules
"""
import functools
import json
import os
import re
import shlex
import subprocess
import sys
import urllib.error
import urllib.parse
import urllib.request
from datetime import datetime, timezone

MIN_PACKAGE_AGE_DAYS = 7
MIN_VERSION_AGE_DAYS = 7
MIN_WEEKLY_DOWNLOADS = 1000
TIMEOUT = 8

# A package manager at command position (start, or after ; & | ( or a newline), optionally behind env
# assignments or sudo, so quoted text that mentions it is not a match.
MANAGER_RE = re.compile(
    r'(?:^|[;&|(\n])\s*(?:\w+=\S+\s+)*(?:sudo\s+)?(?:\S*/)?(npm|npx|pnpm|pnpx|yarn|bun|bunx)\b([^\n]*)')
# Anything that executes project code. Refused outright while the working tree carries a malware signature.
RUN_RE = re.compile(
    r'(?:^|[;&|(\n])\s*(?:\w+=\S+\s+)*(?:sudo\s+)?(?:\S*/)?'
    r'(?:node|nodejs|npm|npx|pnpm|pnpx|yarn|bun|bunx|deno|tsx|ts-node|next|vite|nuxt|nodemon|turbo|nx|corepack)\b')
NAME_RE = re.compile(r'^(@[\w.-]+/)?[\w.-]+$')
REDIRECT_RE = re.compile(r'^\d*[<>&]')
SEGMENT_END_RE = re.compile(r'&&|\|\||;|\|')

# npm accepts every one of these as `install`.
NPM_INSTALL = {'install', 'i', 'in', 'ins', 'inst', 'insta', 'instal', 'isnt', 'isnta', 'isntal', 'isntall', 'add'}
OTHER_INSTALL = {'install', 'i', 'add'}
RUNNERS = {'npx', 'pnpx', 'bunx'}  # these managers run a package directly
EXEC_SUBCOMMANDS = {'npm': {'exec', 'x'}, 'pnpm': {'dlx'}, 'yarn': {'dlx'}, 'bun': {'x'}}
# Global flags that take a separate value, so the value is not mistaken for a subcommand or a package.
VALUE_FLAGS = {'--prefix', '-C', '--dir', '--cwd', '-w', '--workspace', '--filter', '-F', '--registry',
               '--userconfig', '--cache', '--tag', '--package', '-p'}
PROJECT_FLAGS = {'--prefix', '-C', '--dir', '--cwd'}


def decide(decision, reason, tool_input=None):
    output = {
        'hookEventName': 'PreToolUse',
        'permissionDecision': decision,
        'permissionDecisionReason': reason,
    }
    if tool_input is not None:
        output['updatedInput'] = tool_input
    print(json.dumps({'hookSpecificOutput': output}))
    sys.exit(0)


@functools.lru_cache(maxsize=None)
def fetch_json(url):
    req = urllib.request.Request(url, headers={'Accept': 'application/json'})
    with urllib.request.urlopen(req, timeout=TIMEOUT) as resp:
        return json.load(resp)


def age_days(iso):
    when = datetime.fromisoformat(iso.replace('Z', '+00:00'))
    return (datetime.now(timezone.utc) - when).days


def package_name(spec):
    """Return the bare registry name, or None if the spec is not a registry package."""
    if spec.startswith(('.', '/', '~')) or ':' in spec:
        return None
    at = spec.rfind('@')
    name = spec[:at] if at > 0 else spec
    return name if NAME_RE.match(name) else None


def spec_version(spec, name):
    """The version/tag part of a spec ('' when none was given)."""
    return spec[len(name) + 1:] if spec.startswith(name + '@') else ''


def newest_settled_version(meta):
    """Newest stable version published at least MIN_VERSION_AGE_DAYS ago, or None."""
    times = meta.get('time', {})
    settled = [(times[v], v) for v in meta.get('versions', {})
               if '-' not in v and v in times and age_days(times[v]) >= MIN_VERSION_AGE_DAYS]
    return max(settled)[1] if settled else None


def check_package(name, version='', check_latest=True, check_downloads=True):
    """Return (decision, reason) or None when the package looks fine.

    `version` is the version, range or tag being added ('' for none). Only a request without one is
    judged by the age of the latest release. `check_downloads` is False for transitive lockfile entries.
    A reason that starts with "Could not verify" means the lookup failed, not that the package is bad.
    """
    quoted = urllib.parse.quote(name, safe='@')
    try:
        meta = fetch_json(f'https://registry.npmjs.org/{quoted}')
    except urllib.error.HTTPError as exc:
        if exc.code == 404:
            return 'deny', f'"{name}" does not exist on the npm registry (hallucinated or mistyped name).'
        return 'ask', f'Could not verify "{name}" (registry HTTP {exc.code}).'
    except (OSError, ValueError):
        return 'ask', f'Could not verify "{name}" (registry unreachable).'

    problems = []
    too_new = False  # a too-new version is blocked outright; the other problems only ask
    times = meta.get('time', {})
    if times.get('created') and age_days(times['created']) < MIN_PACKAGE_AGE_DAYS:
        problems.append(f'package created {age_days(times["created"])}d ago')
    latest = meta.get('dist-tags', {}).get('latest')
    requested = meta.get('dist-tags', {}).get(version, version)
    if requested and times.get(requested) and age_days(times[requested]) < MIN_VERSION_AGE_DAYS:
        problem = f'requested version {requested} published {age_days(times[requested])}d ago'
        settled = newest_settled_version(meta)
        if settled:
            problem += f' (newest version older than {MIN_VERSION_AGE_DAYS}d: {name}@{settled})'
        problems.append(problem)
        too_new = True
    if check_latest and not version and latest and times.get(latest) and age_days(times[latest]) < MIN_VERSION_AGE_DAYS:
        problem = f'latest version {latest} published {age_days(times[latest])}d ago'
        settled = newest_settled_version(meta)
        if settled:
            problem += f' (newest version older than {MIN_VERSION_AGE_DAYS}d: {name}@{settled})'
        problems.append(problem)
    downloads_unknown = False
    if check_downloads:
        try:
            weekly = fetch_json(f'https://api.npmjs.org/downloads/point/last-week/{quoted}').get('downloads', 0)
            if weekly < MIN_WEEKLY_DOWNLOADS:
                problems.append(f'only {weekly} downloads last week')
        except (OSError, ValueError):
            downloads_unknown = True
    if too_new:
        return 'deny', f'"{name}": ' + '; '.join(problems) + \
            f'. Versions under {MIN_VERSION_AGE_DAYS} days old are blocked; install an older version instead.'
    if problems:
        return 'ask', f'"{name}": ' + '; '.join(problems) + '. Confirm this is the package you intend.'
    if downloads_unknown:
        return 'ask', f'Could not verify "{name}" download count (download stats unavailable).'
    return None


def settled_pin(name, spec):
    """'name@older-version' when an unversioned or @latest install would pull a too-new latest, else None."""
    if spec_version(spec, name) not in ('', 'latest'):
        return None
    try:
        meta = fetch_json(f'https://registry.npmjs.org/{urllib.parse.quote(name, safe="@")}')
    except (OSError, ValueError):
        return None
    latest = meta.get('dist-tags', {}).get('latest')
    times = meta.get('time', {})
    if latest and times.get(latest) and age_days(times[latest]) < MIN_VERSION_AGE_DAYS:
        settled = newest_settled_version(meta)
        return f'{name}@{settled}' if settled else None
    return None


def project_dir(cwd):
    """The npm project npm would use from cwd: the nearest directory with a package.json, else cwd."""
    current = os.path.abspath(cwd)
    while True:
        if os.path.isfile(os.path.join(current, 'package.json')):
            return current
        parent = os.path.dirname(current)
        if parent == current:
            return os.path.abspath(cwd)
        current = parent


def npmrc_settings(path):
    """Settings in an .npmrc file, read like npm does: for a repeated key the last value wins."""
    settings = {}
    try:
        with open(path, encoding='utf-8') as handle:
            lines = handle.read().splitlines()
    except OSError:
        return settings
    for line in lines:
        line = line.strip()
        if not line or line[0] in '#;':
            continue
        key, sep, value = line.partition('=')
        if sep:
            settings[key.strip()] = value.strip().strip('"\'')
    return settings


def scripts_disabled(flags, project):
    for flag in reversed(flags):
        if flag in ('--ignore-scripts', '--ignore-scripts=true'):
            return True
        if flag == '--ignore-scripts=false':
            return False
    return npmrc_settings(os.path.join(project, '.npmrc')).get('ignore-scripts') == 'true'


def infected_tree(event):
    """Output of the worktree scan when it finds malware, else None (also None if the guard is not installed)."""
    root = os.environ.get('CLAUDE_PROJECT_DIR') or event.get('cwd') or os.getcwd()
    scanner = os.path.join(root, 'tools', 'npm-guard', 'scan_incoming.py')
    if not os.path.isfile(scanner):
        return None
    try:
        result = subprocess.run([sys.executable, scanner, '--worktree'], cwd=root, capture_output=True,
                                encoding='utf-8', errors='replace', timeout=60)
    except (OSError, subprocess.TimeoutExpired):
        return None
    return result.stderr.strip() or 'malware signature found' if result.returncode == 1 else None


def parse(manager, tokens):
    """(kind, specs, flags, project override) for one package-manager invocation, or None.

    kind is 'install' or 'run'. Flags that take a value keep it ('--prefix=dir' form)."""
    positional, flags, project, only_args = [], [], None, False
    i = 0
    while i < len(tokens):
        token = tokens[i]
        if only_args or not token.startswith('-'):
            if not REDIRECT_RE.match(token):
                positional.append(token)
        elif token == '--':
            only_args = True
        else:
            key, _, value = token.partition('=')
            if key in VALUE_FLAGS and not value and i + 1 < len(tokens):
                i += 1
                value = tokens[i]
            flags.append(f'{key}={value}' if value else key)
            if key in PROJECT_FLAGS and value:
                project = value
            if key in ('--package', '-p') and value:
                positional.insert(0, value)
        i += 1
    if manager in RUNNERS:
        return ('run', positional[:1], flags, project) if positional else None
    if not positional:
        return ('install', [], flags, project) if manager == 'yarn' else None
    sub, rest = positional[0], positional[1:]
    installs = NPM_INSTALL if manager == 'npm' else OTHER_INSTALL
    if sub in installs:
        return 'install', rest, flags, project
    if sub in EXEC_SUBCOMMANDS.get(manager, ()):
        return ('run', rest[:1], flags, project) if rest else None
    return None


def vet(command, cwd):
    """(denies, asks, swaps) for every install and package run in the command."""
    denies, asks, swaps = [], [], {}

    def record(verdict):
        if verdict:
            (denies if verdict[0] == 'deny' else asks).append(verdict[1])

    for match in MANAGER_RE.finditer(command):
        manager, tail = match.group(1), SEGMENT_END_RE.split(match.group(2))[0]
        try:
            tokens = shlex.split(tail)
        except ValueError:
            asks.append(f'Could not parse "{manager}{tail}"; please review it.')
            continue
        parsed = parse(manager, tokens)
        if parsed is None:
            continue
        kind, specs, flags, project = parsed
        project = project_dir(os.path.join(cwd, project) if project else cwd)
        if kind == 'install' and not scripts_disabled(flags, project):
            asks.append('Dependency lifecycle scripts are enabled. Add --ignore-scripts or set '
                        'ignore-scripts=true in .npmrc so a malicious postinstall cannot run.')
        for spec in specs:
            name = package_name(spec)
            if name is None:
                if kind == 'install':
                    asks.append(f'"{spec}" is a path/URL/git dependency, not a vetted registry package.')
                continue
            if kind == 'run' and os.path.exists(os.path.join(project, 'node_modules', '.bin', name)):
                continue  # runs a binary the project already installed
            pin = settled_pin(name, spec) if kind == 'install' else None
            if pin:
                swaps[spec] = pin
                record(check_package(name, spec_version(pin, name), check_latest=False))
            else:
                record(check_package(name, spec_version(spec, name)))
    return denies, asks, swaps


def main():
    try:
        event = json.load(sys.stdin)
    except ValueError:
        return
    if event.get('tool_name') != 'Bash':
        return
    command = event.get('tool_input', {}).get('command', '')

    if RUN_RE.search(command):
        found = infected_tree(event)
        if found:
            decide('deny', 'npm-guard: refusing to run node/npm/next/etc. because this repository contains malware.\n'
                           + found + '\nDo not run the project. Tell security; undo the update (git reset --hard ORIG_HEAD).')

    denies, asks, swaps = vet(command, event.get('cwd') or os.getcwd())
    if denies:
        decide('deny', '\n'.join(denies + asks))
    if swaps:
        new_command = command
        for spec, pin in swaps.items():
            new_command = re.sub(r'(?<![\w@/.:-])' + re.escape(spec) + r'(?![\w@/.:-])', pin, new_command, count=1)
        asks.append('Latest version is under %d days old, so the install was switched to the newest older '
                    'version: %s' % (MIN_VERSION_AGE_DAYS, ', '.join(swaps.values())))
        decide('ask', '\n'.join(asks), {**event['tool_input'], 'command': new_command})
    if asks:
        decide('ask', '\n'.join(asks))


if __name__ == '__main__':
    main()
