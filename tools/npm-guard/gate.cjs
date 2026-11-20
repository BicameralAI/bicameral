#!/usr/bin/env node
// Cross-platform entry point for the npm-guard checks. npm runs package.json scripts through cmd.exe on
// Windows, where `sh` and `python3` are usually missing, so package.json scripts call this file (Node is
// always there).
//   node tools/npm-guard/gate.cjs --gate     scan the working tree; exit 1 if a malware signature is found
//   node tools/npm-guard/gate.cjs --verify   guard files and the dashboard .npmrc are intact, then scan
//   node tools/npm-guard/gate.cjs --setup    per-clone setup (needs sh: Git Bash on Windows)
//   node tools/npm-guard/gate.cjs --check    report what is set up; exit 1 if something is missing
//
// The checked tree defaults to the repository that contains this file. NPM_GUARD_ROOT points the checks
// at another tree, so CI can run the reviewed base-branch copy of the guard against a pull request.
//
// --gate and --verify fail closed: when Python 3 or the scanner is missing, they exit 1 and the npm script
// that called them does not run.
const { spawnSync } = require('child_process');
const fs = require('fs');
const path = require('path');

const guardDir = __dirname;
const root = path.resolve(process.env.NPM_GUARD_ROOT || path.join(__dirname, '..', '..'));
const mode = process.argv[2] || '--gate';

// The only npm project in the repository. Its .npmrc carries the install-time protections.
const NPMRC = 'apps/bot/dashboard/.npmrc';
const REQUIRED_FILES = [
  'tools/npm-guard/hooks/pre-commit', 'tools/npm-guard/hooks/pre-push', 'tools/npm-guard/hooks/pre-merge-commit',
  'tools/npm-guard/hooks/post-merge', 'tools/npm-guard/hooks/post-rewrite', 'tools/npm-guard/hooks/post-checkout',
  'tools/npm-guard/scan_incoming.py', 'tools/npm-guard/check_dependency_changes.py',
  'tools/npm-guard/registry_check.py',
];
const MIN_RELEASE_AGE_DAYS = 7;

// Read .npmrc the way npm does: comments are skipped, and for a repeated key the last value wins.
function npmrcSettings(text) {
  const settings = {};
  for (const raw of text.split(/\r?\n/)) {
    const line = raw.trim();
    if (!line || line[0] === '#' || line[0] === ';') continue;
    const eq = line.indexOf('=');
    if (eq > 0) settings[line.slice(0, eq).trim()] = line.slice(eq + 1).trim().replace(/^["']|["']$/g, '');
  }
  return settings;
}

// Problems with the dashboard .npmrc, as messages.
function npmrcProblems(text) {
  const s = npmrcSettings(text);
  const problems = [];
  if (s['ignore-scripts'] !== 'true') problems.push('must set ignore-scripts=true');
  const age = Number(s['min-release-age']);
  if (!Number.isInteger(age) || age < MIN_RELEASE_AGE_DAYS) problems.push(`must set min-release-age to ${MIN_RELEASE_AGE_DAYS} or more`);
  for (const key of Object.keys(s)) {
    if (key === 'registry' || key.endsWith(':registry') || key === 'replace-registry-host') {
      problems.push(`must not set ${key} (packages come from registry.npmjs.org only)`);
    }
  }
  return problems;
}

function python() {
  for (const [cmd, pre] of [['python3', []], ['python', []], ['py', ['-3']]]) {
    const r = spawnSync(cmd, [...pre, '-c', 'import sys; sys.exit(sys.version_info < (3, 6))'], { stdio: 'ignore' });
    if (r.status === 0) return [cmd, pre];
  }
  return null;
}

function scan() {
  const scanner = path.join(guardDir, 'scan_incoming.py');
  if (!fs.existsSync(scanner)) {
    console.error(`npm-guard: ${scanner} is missing, so the malware scan cannot run. Not starting.\n` +
      'Restore tools/npm-guard from a reviewed main (git checkout origin/main -- tools/npm-guard).');
    return 1;
  }
  const py = python();
  if (!py) {
    console.error('npm-guard: Python 3.6+ not found (tried python3, python, py -3), so the malware scan cannot ' +
      'run. Not starting.\nInstall Python 3 (https://www.python.org/downloads/) and make sure it is on PATH.');
    return 1;
  }
  const r = spawnSync(py[0], [...py[1], scanner, '--worktree'], { cwd: root, stdio: 'inherit' });
  return r.status === 0 ? 0 : 1;
}

function verify() {
  let bad = 0;
  for (const f of REQUIRED_FILES) {
    if (!fs.existsSync(path.join(root, f))) { console.error('npm-guard: MISSING ' + f); bad = 1; }
  }
  let npmrc = '';
  try { npmrc = fs.readFileSync(path.join(root, NPMRC), 'utf8'); } catch (e) { /* reported below */ }
  for (const problem of npmrcProblems(npmrc)) {
    console.error('npm-guard: ' + NPMRC + ' ' + problem);
    bad = 1;
  }
  if (scan() !== 0) bad = 1;
  if (bad) console.error('npm-guard: the supply-chain guard was removed or weakened, or malware was found.');
  return bad;
}

function findSh() {
  if (spawnSync('sh', ['-c', 'exit 0'], { stdio: 'ignore' }).status === 0) return 'sh';
  for (const base of [process.env.ProgramFiles, process.env['ProgramFiles(x86)'], process.env.LOCALAPPDATA && path.join(process.env.LOCALAPPDATA, 'Programs')]) {
    for (const rel of [['Git', 'bin', 'sh.exe'], ['Git', 'usr', 'bin', 'sh.exe']]) {
      const p = base && path.join(base, ...rel);
      if (p && fs.existsSync(p)) return p;
    }
  }
  return null;
}

let code;
if (mode === '--gate') code = scan();
else if (mode === '--verify') code = verify();
else if (mode === '--setup' || mode === '--check') {
  const sh = findSh();
  if (!sh) { console.error('npm-guard: needs sh (install Git for Windows and use Git Bash)'); code = 1; }
  else code = spawnSync(sh, [path.join(guardDir, 'setup.sh'), ...(mode === '--check' ? ['--check'] : [])], { cwd: root, stdio: 'inherit' }).status ?? 1;
} else { console.error('usage: gate.cjs --gate|--verify|--setup|--check'); code = 2; }
process.exit(code);
