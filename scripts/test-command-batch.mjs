import assert from 'node:assert/strict';
import { fileURLToPath } from 'node:url';
import { createServer } from 'vite';
import { createRenderer, ssrContextKey } from 'vue';
import { mkdtemp, rm, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
const cacheDir = await mkdtemp(join(tmpdir(), 'noryn-batch-ui-'));
const mockPath = join(cacheDir, 'ipc.mjs');
await writeFile(
  mockPath,
  `
export const transport = { calls: [], connect: null, batch: null, healthView: null };
export const isTauri = () => true;
export class Channel {}
export async function invoke(name, args) {
  transport.calls.push({name, args});
  if (name === 'connect_node') return await transport.connect(args);
  if (name === 'command_batch_run') return await transport.batch(args);
  if (name === 'ssh_login_health') return {report:{capturedAt:1,ports:[]},view:transport.healthView};
  if (['command_batch_cancel','connection_close','auth_respond'].includes(name)) return;
  throw new Error('Unexpected IPC: '+name);
}`,
);
const server = await createServer({
  cacheDir,
  optimizeDeps: { noDiscovery: true, include: [] },
  resolve: { alias: { '@tauri-apps/api/core': mockPath } },
  root: fileURLToPath(new URL('..', import.meta.url)),
  server: { middlewareMode: true, hmr: false, watch: null },
});
const tick = () => new Promise((resolve) => setTimeout(resolve, 0));
async function until(check) {
  for (let i = 0; i < 100; i++) {
    if (check()) return;
    await tick();
  }
  throw new Error('Expected state did not arrive');
}
let app;
try {
  const { transport } = await server.ssrLoadModule(mockPath);
  const { store } = await server.ssrLoadModule('/src/composables/useNoryn.ts');
  const component = (await server.ssrLoadModule('/src/components/CommandBatch.vue')).default;
  const command = {
    id: 'command',
    name: 'test',
    workspace: 'private',
    description: '',
    command: 'printf test',
  };
  const node = (id) => ({
    id,
    parentId: null,
    workspace: 'private',
    kind: 'connection',
    protocol: 'ssh',
    name: id,
    host: `${id}.invalid`,
    port: 22,
    initialPath: '',
    notes: '',
    tags: [],
    favorite: false,
  });
  const reset = () => {
    store.unlocked = true;
    store.loading = false;
    store.challenge = null;
    store.authPrompt = null;
    store.view.nodes = ['a', 'b', 'c', 'd'].map(node);
    store.view.nodes.push({ ...node('files'), protocol: 'sftp' });
    store.view.credentials = [{ id: 'account', name: 'Test', username: 'alice', kind: 'password' }];
    store.view.bindings = { a: 'account', b: 'account', c: 'account' };
    store.view.connectionCredentials = {};
    store.view.teams = [];
    store.view.commands = [command];
    store.sessions = [
      {
        ...node('a'),
        id: 'existing-a',
        nodeId: 'a',
        username: 'alice',
        connected: true,
        temporary: false,
        buffer: [],
      },
    ];
    store.activeTab = 'connections';
    transport.healthView = JSON.parse(JSON.stringify(store.view));
  };
  let form;
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
  const mount = () => {
    app = renderer.createApp({
      setup() {
        form = component.setup({ command }, { emit() {}, expose() {} });
        return () => null;
      },
    });
    app.provide(ssrContextKey, { modules: new Set() });
    app.mount({});
  };
  reset();
  mount();
  assert.equal(
    form.targets.value.length,
    4,
    'Closed saved SSH targets must be selectable; file-only targets must not be',
  );
  form.selected.value = ['a', 'b', 'c', 'd'];
  let otpDone, batchDone;
  transport.connect = async (args) => {
    if (args.nodeId === 'c') throw new Error('Authentication rejected');
    assert.equal(args.nodeId, 'b');
    if (!args.trustedFingerprint)
      return {
        type: 'hostKey',
        host: 'b.invalid',
        fingerprint: 'test-fingerprint',
        previous: null,
      };
    assert.equal(args.trustedFingerprint, 'test-fingerprint');
    args.authOutput.onmessage({
      id: 'otp',
      host: 'b.invalid',
      username: 'alice',
      instructions: 'OTP',
      prompts: [{ text: 'Code', echo: false }],
    });
    return await new Promise((resolve) => (otpDone = resolve));
  };
  transport.batch = (args) => {
    assert.deepEqual(args.ids, ['existing-a', 'opened-b']);
    args.output.onmessage({
      id: 'existing-a',
      output: { stdout: 'a output', stderr: '', exitCode: 0, error: null, truncated: false },
    });
    return new Promise((resolve) => (batchDone = resolve));
  };
  const pending = form.run();
  await until(() => store.challenge);
  assert.equal(transport.calls.filter((c) => c.name === 'command_batch_run').length, 0);
  const challenge = store.challenge;
  store.challenge = null;
  const retry = challenge.retry();
  await until(() => otpDone);
  assert.equal(store.authPrompt.id, 'otp', 'OTP must use the normal authentication surface');
  otpDone({ type: 'connected', id: 'opened-b', home: '/home/alice', username: 'alice' });
  await retry;
  await until(() => batchDone);
  assert.equal(
    store.activeTab,
    'connections',
    'Automatic opening must keep the result view selected',
  );
  assert.equal(form.results.value.find((r) => r.id === 'a').output.stdout, 'a output');
  assert.match(form.results.value.find((r) => r.id === 'c').output.error, /rejected/);
  assert.match(form.results.value.find((r) => r.id === 'd').output.error, /Zugangsdaten/);
  batchDone([
    {
      id: 'existing-a',
      output: { stdout: 'a output', stderr: '', exitCode: 0, error: null, truncated: false },
    },
    {
      id: 'opened-b',
      output: { stdout: 'b output', stderr: 'warning', exitCode: 7, error: null, truncated: false },
    },
  ]);
  await pending;
  assert.equal(form.results.value.length, 4);
  assert.equal(form.activeTarget.value, 'a');
  form.selectResult('b');
  assert.equal(form.activeReply.value.output.stdout, 'b output');
  form.selectResult('c');
  assert.match(form.activeReply.value.output.error, /rejected/);
  form.selectResult('unselected');
  assert.equal(form.activeTarget.value, 'c');
  assert.equal(form.results.value.find((r) => r.id === 'b').output.exitCode, 7);
  assert.equal(store.sessions.length, 1, 'New sessions close by default; reused sessions remain');
  assert.equal(store.sessions[0].id, 'existing-a');
  assert.deepEqual(
    transport.calls.filter((c) => c.name === 'connection_close').map((c) => c.args.id),
    ['opened-b'],
  );
  app.unmount();
  app = null;
  reset();
  mount();
  form.selected.value = ['b'];
  let late;
  transport.connect = () => new Promise((resolve) => (late = resolve));
  const batches = transport.calls.filter((c) => c.name === 'command_batch_run').length;
  const cancelled = form.run();
  await until(() => late);
  await form.cancel();
  await cancelled;
  late({ type: 'connected', id: 'late-session', home: '/', username: 'alice' });
  await until(() =>
    transport.calls.some((c) => c.name === 'connection_close' && c.args.id === 'late-session'),
  );
  assert.equal(
    transport.calls.filter((c) => c.name === 'command_batch_run').length,
    batches,
    'Cancellation must never execute a late login',
  );
  assert(!store.sessions.some((s) => s.id === 'late-session'));
  console.log(
    'Command batches: saved closed targets, automatic connection, host key and OTP routing, reuse, per-target outputs/errors, owned-session cleanup and cancelled late login passed.',
  );
} finally {
  app?.unmount();
  await server.close();
  await rm(cacheDir, { recursive: true, force: true });
}
