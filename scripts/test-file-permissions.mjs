import assert from 'node:assert/strict';
import { createServer } from 'vite';
import { createRenderer, nextTick, ssrContextKey } from 'vue';
import { mkdtemp, rm, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';

const cacheDir = await mkdtemp(join(tmpdir(), 'noryn-permissions-'));
const mock = join(cacheDir, 'ipc.mjs');
await writeFile(
  mock,
  `
export const transport = { calls: [], pending: null, snapshot: {path:'/folder/file',mode:0o4755,uid:1000,gid:1000,canChmod:true,canChown:true,fingerprint:'fresh'} };
export const isTauri = () => true;
export async function invoke(name,args) {
  transport.calls.push({name,args});
  if(name === 'remote_transfer_cancel') return;
  if(name !== 'file_permissions') throw Error(name);
  return transport.pending || {...transport.snapshot};
}`,
);
const server = await createServer({
  root: fileURLToPath(new URL('..', import.meta.url)),
  cacheDir,
  optimizeDeps: { noDiscovery: true, include: [] },
  resolve: { alias: { '@tauri-apps/api/core': mock } },
  server: { middlewareMode: true, hmr: false, watch: null },
});
let app;
try {
  const { language } = await server.ssrLoadModule('/src/i18n.ts');
  language.value = 'de';
  const { parseMode, parseId, symbolicMode } = await server.ssrLoadModule('/src/permissions.ts');
  assert.equal(parseMode('755'), 0o755);
  assert.equal(parseMode('0000'), 0);
  for (const invalid of ['', '888', '07555', '75', '7;5', '-1'])
    assert.equal(parseMode(invalid), undefined);
  assert.equal(symbolicMode(0o7745), 'rwsr-Sr-t');
  assert.equal(parseId('0'), 0);
  assert.equal(parseId('4294967294'), 4294967294);
  for (const invalid of ['', '-1', 'root', '4294967295', '1;id'])
    assert.equal(parseId(invalid), undefined);
  const { transport } = await server.ssrLoadModule(mock);
  const component = (await server.ssrLoadModule('/src/components/FilePermissions.vue')).default;
  const renderer = createRenderer({
    createElement: () => ({}),
    createText: () => ({}),
    createComment: () => ({}),
    insert() {},
    remove() {},
    patchProp() {},
    setText() {},
    setElementText() {},
    parentNode: () => null,
    nextSibling: () => null,
  });
  let form,
    done = 0;
  app = renderer.createApp({
    setup() {
      form = component.setup(
        { session: { id: 'session', protocol: 'ssh' }, side: 'remote' },
        {
          emit() {
            done++;
          },
          expose() {},
        },
      );
      return () => null;
    },
  });
  app.provide(ssrContextKey, { modules: new Set() });
  app.mount({});
  await form.open({ path: '/folder/file', permissions: '644' });
  assert.equal(transport.calls.length, 1);
  assert.equal(transport.calls[0].args.change, null, 'Opening only reads fresh metadata');
  await form.save();
  assert.equal(transport.calls.length, 1, 'No unchanged writes');
  assert.equal(form.modeText.value, '4755', 'Fresh native metadata wins over listing');
  form.toggle(0o020, true);
  assert.equal(form.modeText.value, '4775', 'rwx edits preserve special bits');
  form.toggle(0o4000, false);
  assert.equal(form.modeText.value, '775');
  form.modeText.value = '888';
  await form.save();
  assert.equal(transport.calls.length, 1, 'Invalid mode blocked');
  form.modeText.value = '750';
  form.uidText.value = 'root';
  assert.equal(form.change.value, undefined);
  form.uidText.value = '';
  form.gidText.value = '42';
  assert.deepEqual(
    form.change.value,
    { mode: 0o750, uid: null, gid: 42 },
    'Empty owner remains unchanged',
  );
  await form.save();
  assert.equal(transport.calls.at(-1).args.expected, 'fresh');
  assert.equal(done, 1);
  await form.open({ path: '/folder/file' });
  form.modeText.value = '600';
  transport.pending = Promise.reject(Error('stale metadata'));
  await form.save();
  transport.pending = null;
  assert.equal(form.snapshot.value, undefined);
  assert.match(form.error.value, /stale/);
  const count = transport.calls.length;
  await form.save();
  assert.equal(transport.calls.length, count, 'No mutation retry after error');
  await form.load();
  assert.ok(form.snapshot.value, 'Explicit reload restores metadata');
  transport.snapshot = { ...transport.snapshot, mode: null, uid: null, gid: null, canChown: false };
  await form.load();
  assert.equal(form.modeText.value, '');
  form.toggle(0o400, true);
  assert.equal(form.modeText.value, '', 'Do not invent 000 for unknown rights');
  form.modeText.value = '640';
  form.uidText.value = '0';
  assert.deepEqual(
    form.change.value,
    { mode: 0o640, uid: null, gid: null },
    'FTP never sends chown',
  );
  let finish;
  transport.pending = new Promise((resolve) => (finish = resolve));
  const pending = form.save();
  await nextTick();
  const job = transport.calls.at(-1).args.jobId;
  const before = done;
  app.unmount();
  app = null;
  assert.equal(transport.calls.at(-1).name, 'remote_transfer_cancel');
  assert.equal(transport.calls.at(-1).args.transferId, job);
  finish({ ...transport.snapshot, mode: 0o640 });
  await pending;
  assert.equal(done, before, 'Disposed dialog cannot publish late replies');
  console.log(
    'Permissions: octal/rwx/special bits, UID/GID validation, explicit writes, stale reload, unknown FTP modes and close cancellation passed.',
  );
} finally {
  app?.unmount();
  await server.close();
  await rm(cacheDir, { recursive: true, force: true });
}
