import { checkRdpDisplay } from './rdp-display-checks.mjs';
import assert from 'node:assert/strict';
import { createRenderer, ssrContextKey, nextTick } from 'vue';
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
  export const transport = {calls:[],reply:null,view:null,listeners:{}};
  export const isTauri = () => true;
  export const listen = async (name, callback) => { transport.listeners[name] = callback; return () => {}; };
  export class Channel {}
  export async function invoke(name,args) {
    transport.calls.push({name,args});
    if (name === 'plugin:deep-link|get_current') return null;
    if (name === 'node_save') return await transport.nodeSaveReply(args);
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
  resolve: { alias: { '@tauri-apps/api/core': mockPath, '@tauri-apps/api/event': mockPath } },
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
  const { parseConnectionLink, parseFileDrop } =
    await server.ssrLoadModule('/src/connectionLinks.ts');
  const { pendingLinks, linkError } = await server.ssrLoadModule(
    '/src/composables/useConnectionLinks.ts',
  );
  transport.listeners['deep-link://new-url']({
    payload: ['ssh://link-user@host', 'ssh://link-user@host'],
  });
  assert.equal(
    pendingLinks.value.length,
    1,
    'Native links wait for review and duplicates are coalesced',
  );
  transport.listeners['deep-link://new-url']({
    payload: ['ssh://user:never-keep-url-password@host'],
  });
  assert.equal(pendingLinks.value.length, 1);
  assert.ok(!JSON.stringify(pendingLinks.value).includes('never-keep'));
  assert.ok(!linkError.value.includes('never-keep'));
  assert.equal(transport.calls.filter((c) => c.name === 'quick_connect').length, 0);
  pendingLinks.value = [];
  linkError.value = '';
  assert.deepEqual(parseConnectionLink('sftp://alice@example.org:2222/var/my%20files'), {
    protocol: 'sftp',
    host: 'example.org',
    port: 2222,
    username: 'alice',
    initialPath: '/var/my files',
  });
  assert.equal(parseConnectionLink('ssh://user@[::1]').host, '::1');
  assert.equal(parseConnectionLink('rdp://DOMAIN%5Cuser@host').port, 3389);
  assert.equal(parseConnectionLink('ftp://host').port, 21);
  for (const link of [
    'https://example.org',
    'ssh://user:secret@host',
    'ssh://host/rm%20-rf',
    'ssh://host?command=ls',
    'sftp://host/file%0a',
    'ftp://host:65536/',
    'sftp://bad%2fhost/path',
    'sftp://bad%20host/path',
    'rdp://host#ignored',
    'ssh://host\n',
    'sftp://host/invalid%zz',
  ])
    assert.throws(() => parseConnectionLink(link), undefined, link);
  assert.deepEqual(
    parseFileDrop(['# comment', 'file:///tmp/my%20file.txt', '/tmp/my file.txt', '/tmp/new\nline']),
    {
      local: ['/tmp/my file.txt', '/tmp/new\nline'],
      remote: [],
    },
  );
  assert.deepEqual(parseFileDrop(['file:///C:/Users/test/file.txt']).local, [
    'C:/Users/test/file.txt',
  ]);
  assert.equal(parseFileDrop(['ftp://user@host/remote.txt']).remote[0].initialPath, '/remote.txt');
  for (const paths of [
    ['file://other-server/file'],
    ['https://host/file'],
    ['ssh://host'],
    ['ftp://host/'],
    ['relative.txt'],
    Array(257).fill('/tmp/a'),
  ])
    assert.throws(() => parseFileDrop(paths));
  await checkRdpDisplay(server);
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
  terminal.onmessage({ type: 'closed', message: 'Connection reset by peer' });
  const buffered = session.buffer.length;
  terminal.onmessage({ type: 'data', data: btoa('late data') });
  terminal.onmessage({ type: 'closed', message: 'late error' });
  assert.equal(session.connected, false);
  assert.equal(session.endReason, 'Connection reset by peer');
  assert.equal(session.buffer.length, buffered);
  assert.equal(transport.calls.filter((c) => c.name === 'ssh_login_health').length, 1);
  finishHealth({
    report: { capturedAt: 1234, ports: [] },
    view: { ...transport.healthView, localRevision: 999999 },
  });
  await new Promise((resolve) => setTimeout(resolve, 20));
  assert.equal(session.health, undefined, 'Late health replies cannot update a closed session');
  assert.notEqual(
    store.view.localRevision,
    999999,
    'A discarded health result cannot replace the vault view',
  );
  assert.equal(
    transport.calls.filter((c) => c.name === 'quick_connect').length,
    attempts,
    'Session errors never schedule a reconnect',
  );
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
  const { credentialFor } = await server.ssrLoadModule('/src/composables/useNoryn.ts');
  const inline = (await server.ssrLoadModule('/src/components/InlineLogin.vue')).default;
  const folder = { ...node, id: 'folder', name: 'Private folder', kind: 'folder', host: '' };
  const child = { ...node, id: 'child', parentId: 'folder' };
  store.unlocked = true;
  store.view.nodes = [folder, child];
  store.view.connectionCredentials = {};
  store.view.bindings = {};
  transport.nodeSaveReply = async (args) => ({
    ...store.view,
    localRevision: store.view.localRevision + 1,
    connectionCredentials: {
      ...store.view.connectionCredentials,
      [args.node.id]: {
        id: args.node.id,
        name: args.node.name,
        username: args.login.credential.username,
        kind: args.login.credential.kind,
        hasPassword: !!args.login.credential.password,
        teamId: null,
      },
    },
  });
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
  function mountInline(target) {
    let form,
      done = 0;
    const app = renderer.createApp({
      setup() {
        form = inline.setup(
          { node: target },
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
    return { form, app, done: () => done };
  }
  let editor = mountInline(folder);
  editor.form.form.username = 'folder-user';
  editor.form.form.kind = 'key';
  editor.form.form.secret = 'test-only-private-key';
  editor.form.form.password = 'test-only-login-password';
  editor.form.form.passphrase = 'test-only-key-passphrase';
  language.value = 'en';
  await nextTick();
  assert.equal(
    editor.form.form.password,
    'test-only-login-password',
    'Language changes preserve unsaved input',
  );
  language.value = 'de';
  await editor.form.submit();
  const saved = transport.calls.at(-1).args;
  assert.equal(saved.node.kind, 'folder');
  assert.equal(saved.login.mode, 'direct');
  assert.ok(Number.isInteger(saved.expectedRevision));
  assert.equal(editor.done(), 1);
  assert.equal(editor.form.form.secret, '');
  assert.equal(editor.form.form.password, '');
  assert.equal(editor.form.form.passphrase, '');
  assert.equal(credentialFor('child').credential.username, 'folder-user');
  assert.equal(credentialFor('child').inherited, true);
  assert.equal(credentialFor('child').direct, true);
  assert.equal(credentialFor('child').source, 'Private folder');
  assert.ok(!JSON.stringify(store.view).includes('test-only-login-password'));
  editor.app.unmount();
  const quick = (await server.ssrLoadModule('/src/components/QuickConnect.vue')).default;
  let quickForm;
  const callsBeforeLink = transport.calls.length;
  const quickApp = renderer.createApp({
    setup() {
      quickForm = quick.setup(
        { link: parseConnectionLink('sftp://link-user@host:2222/initial') },
        { emit() {}, expose() {} },
      );
      return () => null;
    },
  });
  quickApp.provide(ssrContextKey, { modules: new Set() });
  quickApp.mount({});
  assert.equal(quickForm.node.protocol, 'sftp');
  assert.equal(quickForm.node.initialPath, '/initial');
  assert.equal(quickForm.credential.username, 'link-user');
  assert.equal(
    quickForm.node.username,
    undefined,
    'The URL username belongs to the personal login, never connection metadata',
  );
  assert.equal(
    transport.calls.length,
    callsBeforeLink,
    'Opening a link only prefills the review form',
  );
  quickForm.credential.password = 'unsaved-password';
  language.value = 'en';
  await nextTick();
  assert.equal(quickForm.credential.password, 'unsaved-password');
  assert.equal(quickForm.applyAddress('ssh://user:secret@host'), false);
  assert.equal(quickForm.node.host, 'host', 'Rejected links cannot overwrite the form');
  quickApp.unmount();
  assert.equal(quickForm.credential.password, '');
  language.value = 'de';
  console.log(
    'Connection links: review without login, unsafe URL rejection, native paths/URI drops, frame bounds and extended RDP keys passed.',
  );
  store.view.credentials = [
    {
      id: 'personal',
      name: 'Saved account',
      username: 'saved-user',
      kind: 'password',
      hasPassword: true,
      teamId: null,
    },
  ];
  store.view.bindings.child = 'personal';
  assert.equal(
    credentialFor('child').credential.username,
    'saved-user',
    'A closer saved binding overrides an inline folder login',
  );
  delete store.view.bindings.child;
  editor = mountInline(folder);
  assert.equal(editor.form.form.username, 'folder-user');
  assert.equal(editor.form.form.secret, '');
  assert.equal(editor.form.form.password, '', 'Stored secrets never return to the form');
  transport.nodeSaveReply = async () => {
    throw Error('Der Tresor wurde während der Bearbeitung aktualisiert.');
  };
  editor.form.form.password = 'unsaved-draft';
  await editor.form.submit();
  assert.match(editor.form.error.value, /aktualisiert/);
  assert.equal(
    editor.form.form.password,
    'unsaved-draft',
    'A failed/stale save preserves user input',
  );
  editor.app.unmount();
  assert.equal(editor.form.form.password, '', 'Closing clears the draft');
  editor = mountInline({ ...child, workspace: 'ops' });
  editor.form.form.username = 'never-share';
  editor.form.form.password = 'never-send';
  const before = transport.calls.length;
  await editor.form.submit();
  assert.equal(transport.calls.length, before, 'Team nodes cannot submit inline secrets');
  editor.app.unmount();
  console.log(
    'Inline logins: private folder form, inheritance and overrides, local-only view, empty secret edits, language changes, stale save and cleanup passed.',
  );
  console.log(
    'Quick connect: trust/cancel/OTP routing, temporary lifecycle, save without reconnect, late reply and lock cleanup passed.',
  );
} finally {
  await server.close();
  await rm(cacheDir, { recursive: true, force: true });
}
