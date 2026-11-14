// Real binary + Git protocol regression. Run only with an isolated environment.
// Herdr/GPUI/core are never contacted; only the JS transport is adapted to execFile.
import assert from 'node:assert/strict';
import * as fs from 'node:fs/promises';
import path from 'node:path';
import os from 'node:os';
import { spawnSync } from 'node:child_process';
import { register } from 'node:module';
import { fileURLToPath } from 'node:url';
const binary = path.resolve(process.argv[2] ?? 'target/debug/herdr-nvim.exe');
const repo = fileURLToPath(new URL('../..', import.meta.url));
const root = await fs.mkdtemp(path.join(os.tmpdir(), 'hn-native-protocol-'));
console.log(`fixture root: ${root}`);
const env = Object.fromEntries(Object.entries(process.env).map(([k, v]) => [k.toUpperCase(), v]));
for (const key of ['HERDR_NVIM_HOST', 'HERDR_NVIM_SELF', 'HERDR_NVIM_PLUGIN_ROOT', 'HERDR_APP_HOME', 'HERDR_APP_BIN', 'HERDR_NVIM_INSTANCE', 'HERDR_NVIM_CORE_BINDING', 'HERDR_NVIM_HC', 'HC_SESSION', 'HC_SOCKET', 'HC_TERMINAL_ID', 'HC_SCOPE']) delete env[key];
function run(exe, args, extra = {}) {
  const r = spawnSync(exe, args, { env, encoding: 'utf8', timeout: 30000, ...extra });
  assert.ifError(r.error);
  return { code: r.status, stdout: r.stdout, stderr: r.stderr };
}
function ok(r) { assert.equal(r.code, 0, r.stderr); return r.stdout; }
const fake = path.join(root, 'fake-core.exe');
ok(run('rustc.exe', [path.join(repo, 'tests/support/fake-core.rs'), '-o', fake]));
const git = ok(run('where.exe', ['git.exe'])).trim().split(/\r?\n/)[0];
const nvim = ok(run('where.exe', ['nvim.exe'])).trim().split(/\r?\n/)[0];
// Exercise the runtime with only native Git and Windows tools on PATH. No
// sh/cat/cp fallback can accidentally be supplied by Git Bash or the test host.
env.PATH = [path.dirname(git), path.join(env.SYSTEMROOT, 'System32')].join(path.delimiter);
for (const tool of ['sh.exe', 'cat.exe', 'cp.exe']) assert.notEqual(run('where.exe', [tool]).code, 0);
for (const key of ['HOME', 'USERPROFILE', 'APPDATA', 'LOCALAPPDATA', 'XDG_CONFIG_HOME', 'XDG_DATA_HOME', 'XDG_STATE_HOME', 'XDG_CACHE_HOME', 'XDG_RUNTIME_DIR', 'TEMP', 'TMP', 'HERDR_NVIM_RUNTIME_DIR', 'HERDR_NVIM_STATE_DIR']) {
  env[key] = path.join(root, 'isolated', key); await fs.mkdir(env[key], { recursive: true });
}
env.HERDR_NVIM_CONFIG = path.join(root, 'absent-config.toml');
const home = path.join(root, 'sidecar home 空 格');
await fs.mkdir(home);
const core = JSON.parse(ok(run(binary, ['app-core-binding'], { env: { ...env, HERDR_NVIM_HC: fake, HERDR_APP_HOME: home, HC_SESSION: 'protocol-only' } })));
const entry = path.join(root, 'native-entry.exe');
await fs.copyFile(binary, entry);
await fs.writeFile(entry.replace(/\.exe$/, '.app.json'), JSON.stringify({ plugin_root: repo, app_home: home, app_bin: fake, core }));
const folder = path.join(root, 'repo 空 格');
await fs.mkdir(folder);
ok(run('git', ['init', '-q', folder]));
await fs.writeFile(path.join(folder, '.gitignore'), 'ignored dir/\n空 白.txt\n');
await fs.mkdir(path.join(folder, 'ignored dir'));
await fs.writeFile(path.join(folder, 'ignored dir', 'hidden.txt'), 'hidden');
await fs.writeFile(path.join(folder, '空 白.txt'), 'ignored');
await fs.writeFile(path.join(folder, 'visible 空 格.md'), '# native text');
// An identical binary without sidecar must reject the app-exclusive command.
// This pair cannot pass if entry::configure is disabled or HOST is inherited.
const raw = path.join(root, 'raw.exe'); await fs.copyFile(binary, raw);
assert.notEqual(run(raw, ['app-files', 'list', folder]).code, 0);
const listing = JSON.parse(ok(run(entry, ['app-files', 'list', folder])));
await fs.writeFile(path.join(root, 'actual-list.json'), JSON.stringify(listing, null, 2));
assert.deepEqual(listing.ignored.sort(), ['ignored dir', '空 白.txt']);
assert.ok(!listing.entries.some(e => e.name === '.git'));
register(`data:text/javascript,${encodeURIComponent(`
  export async function resolve(spec, ctx, next) {
    if (["herdr", "gpui-kit", "gpui-base"].includes(spec)) return { url: ${JSON.stringify(new URL('./herdr-stub.mjs', import.meta.url).href)}, shortCircuit: true };
    return next(spec, ctx);
  }`)}`);
const { app } = await import('./herdr-stub.mjs');
let calls = [];
app.exec = async (command, args, options = {}) => {
  assert.ok(!['sh', 'cat', 'cp'].includes(command));
  calls.push([command, args]);
  return run(command === 'git' ? command : entry, args, options);
};
const { TreeModel } = await import('../mods/herdr-nvim/tree-model.js');
const tree = new TreeModel();
tree.reset(folder);
await tree.reload();
assert.deepEqual(tree.rows().map(r => r.name), ['visible 空 格.md']);
calls = [];
assert.deepEqual(await tree.reloadChanged([path.join(folder, 'ignored dir', 'hidden.txt'), path.join(folder, '空 白.txt')]), { marks: 'none' });
assert.equal(calls.length, 0, 'ignored watcher events must not list or refresh Git');
tree.showIgnored = true;
assert.ok(tree.rows().some(r => r.name === 'ignored dir'));
console.log('PASS real Git NUL listing -> tree hide/event filter');
const plain = path.join(root, 'non-git');
await fs.mkdir(plain);
await fs.writeFile(path.join(plain, 'a.txt'), 'a');
assert.deepEqual(JSON.parse(ok(run(entry, ['app-files', 'list', plain]))).ignored, []);
await fs.writeFile(path.join(folder, '.gitignore'), 'no-matching-name\n');
assert.deepEqual(JSON.parse(ok(run(entry, ['app-files', 'list', folder]))).ignored, []);
const broken = run(entry, ['app-files', 'list', folder], { env: { ...env, GIT_CONFIG_COUNT: '1', GIT_CONFIG_KEY_0: 'core.bare', GIT_CONFIG_VALUE_0: 'not-a-bool' } });
assert.notEqual(broken.code, 0, 'real Git errors must not become an empty ignored list');
assert.match(broken.stderr, /git|Git/);
console.log('PASS no-match/non-repo/error distinction');
await fs.mkdir(path.join(folder, '~dir'));
for (const name of ['~notes.md', '~dir/a.rs']) await fs.writeFile(path.join(folder, name), 'literal repo content');
for (const [query, expected] of [['~notes', '~notes.md'], ['a.rs', '~dir/a.rs']]) {
  const found = JSON.parse(ok(run(entry, ['app-find', '--root', folder, '--query', query]))).paths;
  assert.ok(found.includes(expected), `app-find lost literal repo path: ${JSON.stringify(found)}`);
  assert.ok(found.every(p => !path.isAbsolute(p)), 'app-find must not escape to a HOME-derived absolute path');
  assert.equal(await fs.readFile(path.join(folder, expected), 'utf8'), 'literal repo content');
}
console.log('PASS real app-find literal tilde names');
const { load } = await import('../mods/herdr-nvim/preview.js');
const textFile = path.join(folder, 'visible 空 格.md');
assert.deepEqual(await load(textFile), { kind: 'markdown', text: '# native text' });
const png = Buffer.from('iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAusB9Wl6zZcAAAAASUVORK5CYII=', 'base64');
const image = path.join(folder, '空 格.png'); await fs.writeFile(image, png);
const preview = await load(image);
assert.equal(preview.kind, 'image');
assert.match(preview.src, /^mods\/herdr-nvim\/\.cache\/[^/]+\.png$/);
assert.deepEqual(await fs.readFile(path.join(home, preview.src)), png, 'cache must be in sidecar home, not inherited home');
const contextFile = path.join(root, 'child-context.json');
ok(run(entry, ['app-reveal', '--toggle'], { env: { ...env, HERDR_TAB_ID: 'sp:private', HNV_CONTEXT_PROBE: path.join(repo, 'tests/support/sidecar-context.lua'), HNV_PROBE_NVIM: nvim, HNV_PROBE_TEXT: textFile, HNV_CONTEXT_OUTPUT: contextFile } }));
const child = JSON.parse(await fs.readFile(contextFile, 'utf8'));
for (const [key, expected] of Object.entries({ HERDR_NVIM_SELF: entry, HERDR_NVIM_PLUGIN_ROOT: repo, HERDR_APP_BIN: fake, HERDR_APP_HOME: home })) assert.equal(path.resolve(child[key]), path.resolve(expected), key);
assert.equal(child.HERDR_NVIM_HOST, 'app');
assert.equal(child.HC_SESSION, 'protocol-only');
assert.equal(child.HC_SOCKET, null);
assert.equal(child.text, '# native text', 'real nvim exec.self() must re-enter the native sidecar');
console.log('PASS sidecar text/image cache + APP_BIN -> real nvim plugin/SELF context (no sh/cat/cp)');
// Stress NUL streams beyond Windows argv / pipe capacity.
const long = path.join(root, 'long names'); await fs.mkdir(long);
ok(run('git', ['init', '-q', long]));
await fs.writeFile(path.join(long, '.gitignore'), '*.ignored\n');
for (let i = 0; i < 499; i++) await fs.writeFile(path.join(long, `${i}-${'x'.repeat(100)}.ignored`), '');
assert.equal(JSON.parse(ok(run(entry, ['app-files', 'list', long]))).ignored.length, 499);
console.log('PASS 499 long names over NUL stdin/stdout, not argv');
console.log('native protocol checks: 5 passed');
