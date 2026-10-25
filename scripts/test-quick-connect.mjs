import assert from 'node:assert/strict';
import { mkdtemp, writeFile, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { createServer } from 'vite';
import { fileURLToPath } from 'node:url';
const cacheDir = await mkdtemp(join(tmpdir(), 'noryn-quick-test-'));
const mockPath = join(cacheDir, 'ipc.mjs');
await writeFile(
  mockPath,
  `
  export const transport = {calls:[],reply:null,view:null};
  export const isTauri = () => true;
  export class Channel {}
  export async function invoke(name,args) {
    transport.calls.push({name,args});
    if (name === 'quick_connect') return await transport.reply(args);
    if (name === 'ssh_login_health') return transport.healthReply ? await transport.healthReply(args) : {report: {capturedAt: 1, ports: []}, view: transport.healthView};
    if (name === 'quick_save') {
      const node = {...transport.node,id:'persisted',name:args.name,parentId:args.parentId};
      return {node,view:{...transport.view,localRevision:transport.view.localRevision+1,nodes:[...transport.view.nodes,node]}};
    }
    if (['connection_close','vault_lock'].includes(name)) return;
    throw new Error('Unexpected IPC: '+name);
  }
`,
);
const server = await createServer({
  cacheDir,
  optimizeDeps: { noDiscovery: true, include: [] },
  resolve: { alias: { '@tauri-apps/api/core': mockPath } },
  root: fileURLToPath(new URL('..', import.meta.url)),
  server: { middlewareMode: true, hmr: false, watch: null },
});
try {
  const { language } = await server.ssrLoadModule('/src/i18n.ts');
  language.value = 'de';
  const { transport } = await server.ssrLoadModule(mockPath);
  const { store, connectQuick, cancelHostKey, saveQuickSession, lock } = await server.ssrLoadModule(
    '/src/composables/useNoryn.ts',
  );
  const { emptyCredential } = await server.ssrLoadModule('/src/connectionLogins.ts');
  store.unlocked = true;
  transport.healthView = JSON.parse(JSON.stringify(store.view));
  const node = {
    id: 'transient',
    workspace: 'private',
    parentId: null,
    kind: 'connection',
    name: 'test-host',
    host: 'localhost',
    port: 22,
    protocol: 'ssh',
    initialPath: '',
    tags: [],
    notes: '',
    favorite: false,
  };
  transport.node = node;
  const login = () => ({
    credentialId: null,
    credential: { ...emptyCredential(), username: 'alice', password: 'test-only-password' },
  });
  const challenge = {
    type: 'hostKey',
    fingerprint: 'test-fingerprint',
    previous: null,
    host: 'localhost',
  };
  const connected = {
    type: 'connected',
    id: 'active-session',
    home: '/home/alice',
    username: 'alice',
  };
  transport.reply = async () => challenge;
  let done = 0,
    cancelled = 0;
  let request = login();
  await connectQuick(
    node,
    request,
    () => done++,
    () => cancelled++,
  );
  assert.equal(store.view.nodes.length, 0);
  assert.equal(store.view.credentials.length, 0);
  assert.equal(store.sessions.length, 0);
  assert.equal(done, 0);
  assert.equal(
    request.credential.password,
    'test-only-password',
    'Trust retry needs the ephemeral login',
  );
  await cancelHostKey();
  assert.equal(request.credential.password, '', 'Cancelling clears the ephemeral request');
  assert.equal(cancelled, 1);
  request = login();
  await connectQuick(
    node,
    request,
    () => done++,
    () => cancelled++,
  );
  const retry = store.challenge.retry;
  store.challenge = null;
  transport.reply = async (args) => {
    assert.equal(args.trustedFingerprint, 'test-fingerprint');
    assert.equal(args.request.credential.password, 'test-only-password');
    args.authOutput.onmessage({
      id: 'otp',
      name: 'OTP',
      instructions: '',
      prompts: [{ text: 'Code', echo: false }],
    });
    return connected;
  };
  let finishHealth;
  transport.healthReply = () => new Promise((resolve) => (finishHealth = resolve));
  await retry();
  assert.equal(done, 1);
  assert.equal(request.credential.password, '');
  assert.equal(store.authPrompt, null);
  assert.equal(store.sessions[0].temporary, true);
  assert.equal(store.view.nodes.length, 0, 'Connecting never creates a saved node');
  transport.view = JSON.parse(JSON.stringify(store.view));
  const session = store.sessions[0];
  await saveQuickSession(session, 'Saved host', null);
  assert.equal(session.id, 'active-session', 'Saving must preserve the live transport');
  assert.equal(session.nodeId, 'persisted');
  assert.equal(session.temporary, false);
  assert.equal(store.view.nodes.length, 1);
  await assert.rejects(() => saveQuickSession(session, 'Duplicate', null), /bereits gespeichert/);
  assert.equal(transport.calls.filter((c) => c.name === 'quick_save').length, 1);
  const terminal = transport.calls.filter((c) => c.name === 'quick_connect').at(-1).args.output;
  const attempts = transport.calls.filter((c) => c.name === 'quick_connect').length;
  terminal.onmessage({type: 'closed', message: 'Connection reset by peer'});
  const buffered = session.buffer.length;
  terminal.onmessage({type: 'data', data: btoa('late data')});
  terminal.onmessage({type: 'closed', message: 'late error'});
  assert.equal(session.connected, false);
  assert.equal(session.endReason, 'Connection reset by peer');
  assert.equal(session.buffer.length, buffered);
  assert.equal(transport.calls.filter((c) => c.name === 'ssh_login_health').length, 1);
  finishHealth({report: {capturedAt: 1234, ports: []}, view: {...transport.healthView, localRevision: 999999}});
  await new Promise((resolve) => setTimeout(resolve, 20));
  assert.equal(session.health, undefined, 'Late health replies cannot update a closed session');
  assert.notEqual(store.view.localRevision, 999999, 'A discarded health result cannot replace the vault view');
  assert.equal(transport.calls.filter((c) => c.name === 'quick_connect').length, attempts, 'Session errors never schedule a reconnect');
  request = login();
  transport.reply = async () => {
    throw new Error('Authentication cancelled');
  };
  await assert.rejects(
    () =>
      connectQuick(
        node,
        request,
        () => done++,
        () => cancelled++,
      ),
    /cancelled/,
  );
  assert.equal(request.credential.password, '');
  let finish;
  transport.reply = () => new Promise((resolve) => (finish = resolve));
  request = login();
  const pending = connectQuick(
    node,
    request,
    () => done++,
    () => cancelled++,
  );
  store.unlocked = false;
  finish({ ...connected, id: 'late-session' });
  await assert.rejects(() => pending, /gesperrt/);
  assert.equal(request.credential.password, '');
  assert.equal(transport.calls.at(-1).name, 'connection_close');
  assert.equal(transport.calls.at(-1).args.id, 'late-session');
  store.unlocked = true;
  request = login();
  transport.reply = async () => challenge;
  await connectQuick(
    node,
    request,
    () => done++,
    () => cancelled++,
  );
  await lock();
  assert.equal(request.credential.password, '');
  assert.equal(store.challenge, null);
  assert.equal(store.sessions.length, 0);
  console.log(
    'Quick connect: trust/cancel/OTP routing, temporary lifecycle, save without reconnect, late reply and lock cleanup passed.',
  );
} finally {
  await server.close();
  await rm(cacheDir, { recursive: true, force: true });
}
