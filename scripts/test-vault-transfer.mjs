import assert from 'node:assert/strict';
import { fileURLToPath } from 'node:url';
import { createServer } from 'vite';
import { createRenderer, nextTick, ssrContextKey } from 'vue';
import { mkdtemp, rm, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';

// Exercise the real form controller, with no browser, disk dialog, secrets or server.
const cacheDir = await mkdtemp(join(tmpdir(), 'noryn-transfer-test-'));
const mockPath = join(cacheDir, 'ipc.mjs');
await writeFile(mockPath, `
  export const transport = { calls: [], view: null, dialogs: 0 };
  export const isTauri = () => true;
  export class Channel {}
  export async function open() { transport.dialogs++; return 'test-only.noryn'; }
  export async function save() { transport.dialogs++; return 'test-only.noryn'; }
  export async function invoke(name, args) {
    transport.calls.push({ name, args });
    if (name === 'vault_import_preview') return { id: 'test-preview', summary: {
      scope: {kind:'private'}, label:'Privater Tresor', nodes:2, credentials:2, teams:0,
      replacedNodes:1, replacedCredentials:0,
      sources:[{workspace:'private', name:'Privater Tresor', nodes:2, credentials:2, agentCredentials:1}]
    }};
    if (name === 'vault_import_apply') return {...transport.view, localRevision: ++transport.view.localRevision};
    if (name === 'vault_import_cancel') return;
    throw new Error('Unexpected IPC: ' + name);
  }
`);
const server = await createServer({
  cacheDir,
  optimizeDeps: { noDiscovery: true, include: [] },
  resolve: { alias: { '@tauri-apps/api/core': mockPath, '@tauri-apps/plugin-dialog': mockPath } },
  root: fileURLToPath(new URL('..', import.meta.url)),
  server: { middlewareMode: true, hmr: false, watch: null },
});
let app;
try {
  const { language } = await server.ssrLoadModule('/src/i18n.ts');
  language.value = 'de';
  const { transport } = await server.ssrLoadModule(mockPath);
  const { store } = await server.ssrLoadModule('/src/composables/useNoryn.ts');
  store.view = {
    ...store.view,
    teams: [{id:'restore', name:'Restore Team', memberName:'Alice', serverUrl:'https://sync.example.test', revision:null, dirty:false, liveSync:false}],
  };
  transport.view = JSON.parse(JSON.stringify(store.view));
  const component = (await server.ssrLoadModule('/src/components/VaultBackup.vue')).default;
  let form;
  // A no-op renderer supplies Vue's normal component/effect lifecycle. No UI is driven here.
  const renderer = createRenderer({
    createElement: () => ({}), createText: () => ({}), createComment: () => ({}),
    insert() {}, remove() {}, patchProp() {}, setText() {}, setElementText() {},
    parentNode: () => null, nextSibling: () => null,
  });
  app = renderer.createApp({
    setup(props, context) { form = component.setup(props, context); return () => null; },
  });
  app.provide(ssrContextKey, { modules: new Set() });
  app.mount({});
  form.start('export');
  form.password.value = 'asdf';
  await form.exportVault();
  assert.match(form.error.value, /mindestens 12/);
  assert.equal(transport.dialogs, 0, 'Short passwords must show an inline error before opening a dialog');
  await form.close();
  async function inspect() {
    form.start('import');
    form.file.value = 'test-only.noryn';
    form.password.value = 'test-only-backup-password';
    await form.inspect();
    await nextTick();
    assert.equal(form.password.value, '', 'The password must be cleared after decryption');
  }
  await inspect();
  form.importTarget.value = 'restore'; // A valid team ID, not the restore sentinel.
  await nextTick();
  assert.equal(form.restoring.value, false);
  assert.equal(form.includeCredentials.value, false);
  form.includeCredentials.value = true;
  await nextTick();
  form.confirmed.value = true;
  form.importTarget.value = 'private';
  await nextTick();
  assert.equal(form.includeCredentials.value, false, 'Changing the destination revokes sharing');
  assert.equal(form.confirmed.value, false, 'The new destination must be confirmed again');
  form.importTarget.value = 'restore';
  await nextTick();
  form.confirmed.value = true;
  await form.applyImport();
  let call = transport.calls.filter(c => c.name === 'vault_import_apply').at(-1);
  assert.deepEqual(call.args.destination, {sourceWorkspace:'private', targetWorkspace:'restore', includeCredentials:false});
  await inspect();
  form.importTarget.value = 'restore';
  await nextTick();
  form.includeCredentials.value = true;
  await nextTick();
  form.confirmed.value = true;
  await form.applyImport();
  call = transport.calls.filter(c => c.name === 'vault_import_apply').at(-1);
  assert.equal(call.args.destination.includeCredentials, true);
  console.log('Vault import: visible password validation, default-private credentials, renewed destination consent and IPC mapping verified.');
} finally {
  app?.unmount();
  await server.close();
  await rm(cacheDir, { recursive: true, force: true });
}
