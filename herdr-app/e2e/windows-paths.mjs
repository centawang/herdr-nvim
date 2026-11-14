// Pure path/watcher regression, not evidence of ConPTY or GPUI behavior.
import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import { register } from 'node:module';
import { wire, normalize, dirname, join, inside, relative, same } from '../mods/herdr-nvim/paths.js';
assert.equal(wire(String.raw`C:\Users\空 格\a.md`), 'C:/Users/空 格/a.md');
assert.equal(wire(String.raw`\\?\C:\repo\a`), 'C:/repo/a');
assert.equal(wire(String.raw`\\?\UNC\server\share\a`), '//server/share/a');
assert.equal(wire(String.raw`/tmp/legal\name`), String.raw`/tmp/legal\name`);
for (const root of ['/', 'C:/', '//server/share']) {
  assert.equal(dirname(root), root);
  assert.equal(normalize(`${root}/../../..`), root);
  assert.equal(join(root, 'a/b'), root.replace(/\/$/, '') + '/a/b');
  assert.equal(relative(join(root, 'a/b'), root), 'a/b');
  assert.ok(inside(join(root, 'a'), root));
}
assert.equal(join('C:/repo', '../空 格/a'), 'C:/空 格/a');
assert.equal(join('C:/repo', String.raw`src\a.rs`), 'C:/repo/src/a.rs');
assert.equal(join('/tmp', String.raw`legal\name`), String.raw`/tmp/legal\name`);
for (const name of [String.raw`C:\notes.md`, String.raw`\\notes.md`]) {
  assert.equal(join('/repo', name), `/repo/${name}`, 'Unix entry names must not choose their own Windows path style');
}
assert.ok(!inside('C:/repo2/a', 'C:/repo'));
assert.ok(!inside('//server/share2/a', '//server/share'));
assert.ok(!same('C:/A', 'C:/a'), 'do not flatten case-sensitive directories');
assert.ok(same(String.raw`C:\repo\a`, 'C:/repo/a'));
assert.equal(await readFile(new URL('../mods/herdr-nvim-header/paths.js', import.meta.url), 'utf8'), await readFile(new URL('../mods/herdr-nvim/paths.js', import.meta.url), 'utf8'), 'header helper must stay in sync');
register(`data:text/javascript,${encodeURIComponent(`
  export async function resolve(spec, ctx, next) {
    if (["herdr", "gpui-kit", "gpui-base"].includes(spec)) return { url: ${JSON.stringify(new URL('./herdr-stub.mjs', import.meta.url).href)}, shortCircuit: true };
    return next(spec, ctx);
  }`)}`);
const { app } = await import('./herdr-stub.mjs');
const { TreeModel } = await import('../mods/herdr-nvim/tree-model.js');
let listed = [];
app.exec = async (_, args) => {
  if (args[0] === 'app-files') {
    listed.push(args[2]);
    return { code: 0, stdout: JSON.stringify({ entries: [{ name: 'a.md', dir: false }], ignored: [] }) };
  }
  return { code: 1, stdout: '', stderr: '' };
};
const tree = new TreeModel();
tree.reset('C:\\');
await tree.reload();
assert.equal(tree.rows()[0].abs, 'C:/a.md');
listed = [];
await tree.reloadChanged([String.raw`C:\a.md`]);
assert.deepEqual(listed, ['C:/']);
assert.equal(await tree.expandTo(String.raw`C:\a.md`), true);
const { load } = await import('../mods/herdr-nvim/preview.js');
app.exec = async (command, args) => {
  assert.ok(!['sh', 'cat', 'cp'].includes(command));
  assert.deepEqual(args, ['app-files', 'text', 'C:/outside home/空 格.md']);
  return { code: 0, stdout: JSON.stringify({ text: '# native' }), stderr: '' };
};
assert.deepEqual(await load('C:/outside home/空 格.md'), { kind: 'markdown', text: '# native' });
app.exec = async (_, args) => {
  assert.equal(args[1], 'image');
  return { code: 0, stdout: JSON.stringify({ src: 'mods/herdr-nvim/.cache/abcd1234.png' }), stderr: '' };
};
assert.deepEqual(await load('C:/outside home/a.png'), { kind: 'image', src: 'mods/herdr-nvim/.cache/abcd1234.png' });
app.exec = async () => ({ code: 0, stdout: JSON.stringify({ src: 'mods/herdr-nvim/.cache/../../other.png' }), stderr: '' });
assert.match((await load('C:/a.png')).error, /Invalid image cache/);
app.exec = async () => ({ code: 1, stdout: '', stderr: 'too big to preview' });
assert.match((await load('C:/large.png')).error, /too big/);
// Exact Unix literal names through rows, git marks, find and operation argv.
// This is a JS contract test on Windows, not Unix filesystem execution.
const names = [String.raw`C:\notes.md`, String.raw`\\notes.md`];
let opened;
app.exec = async (_, args) => {
  if (args[0] === 'app-files') return { code: 0, stdout: JSON.stringify({ entries: names.map(name => ({ name, dir: false })), ignored: [] }) };
  if (args[0] === 'rev-parse') return { code: 0, stdout: '/repo\n' };
  if (args.includes('status')) return { code: 0, stdout: names.map(n => `?? ${n}\0`).join('') };
  if (args[0] === 'app-find') return { code: 0, stdout: JSON.stringify({ paths: names }) };
  if (args[0] === 'open') { opened = args; return { code: 0, stdout: '{}' }; }
  throw new Error(`unexpected argv ${args}`);
};
const unix = new TreeModel(); unix.reset('/repo'); await unix.reload();
const { request, requestFor, findFiles } = await import('../mods/herdr-nvim/ops.js');
const { Tree } = await import('../mods/herdr-nvim/tree-view.js');
const view = { filter: { root: '/repo', results: await findFiles('/repo', 'notes') }, where: () => ({ tab: 'private-test' }) };
assert.deepEqual(Tree.prototype.rows.call(view).map(r => r.abs), names.map(n => `/repo/${n}`));
for (const name of names) {
  const row = unix.rows().find(r => r.name === name), expected = `/repo/${name}`;
  assert.equal(row.abs, expected);
  assert.equal(unix.marks[expected], 'A');
  assert.equal(requestFor({ type: 'copy', from: row.abs, base: '/repo' }, name).to, expected);
  assert.deepEqual(request.delete([row.abs]).paths, [expected]);
  assert.equal(request.copy(row.abs, '/repo/copy').path, expected);
  await Tree.prototype.openRow.call(view, row, { notify() {} });
  assert.equal(opened[2], expected);
}
console.log('windows paths/watcher/native preview + Unix literal row/open/copy/delete/git/find checks: ok');
