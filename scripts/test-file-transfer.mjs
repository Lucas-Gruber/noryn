import assert from 'node:assert/strict';
import { fileURLToPath } from 'node:url';
import { createServer } from 'vite';
import { createRenderer, nextTick, reactive, ssrContextKey } from 'vue';
import { mkdtemp, rm, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';

// Exercise the actual queue/dialog controller without filesystem or network access.
const cacheDir = await mkdtemp(join(tmpdir(), 'noryn-file-transfer-'));
const mockPath = join(cacheDir, 'ipc.mjs');
await writeFile(mockPath, `
  export const transport = { calls: [], pending: null, dialogs: [], destination: '/downloads' };
  export const isTauri = () => true;
  export class Channel { onmessage = () => {}; }
  export const getCurrentWebview = () => ({ onDragDropEvent: async () => () => {} });
  export async function open(options) { transport.dialogs.push(options); return transport.destination; }
  export async function save() { throw new Error('A directory must not use the file save dialog'); }
  export async function invoke(name, args) {
    transport.calls.push({name, args});
    if (name === 'remote_transfer') return new Promise((resolve, reject) => transport.pending = {resolve, reject, args});
    if (name === 'remote_transfer_cancel') { transport.pending.reject('Übertragung abgebrochen'); return; }
    throw new Error('Unexpected IPC: ' + name);
  }
`);
const server = await createServer({
  cacheDir,
  optimizeDeps: { noDiscovery: true, include: [] },
  resolve: { alias: { '@tauri-apps/api/core': mockPath, '@tauri-apps/api/webview': mockPath, '@tauri-apps/plugin-dialog': mockPath } },
  root: fileURLToPath(new URL('..', import.meta.url)),
  server: { middlewareMode: true, hmr: false, watch: null },
});
const originalWindow = globalThis.window;
globalThis.window = { addEventListener() {}, removeEventListener() {} };
let app;
const settle = () => new Promise(resolve => setImmediate(resolve));
try {
  const { language } = await server.ssrLoadModule('/src/i18n.ts');
  language.value = 'de';
  const { transport } = await server.ssrLoadModule(mockPath);
  const component = (await server.ssrLoadModule('/src/components/FileWorkspace.vue')).default;
  let form;
  const renderer = createRenderer({
    createElement: () => ({}), createText: () => ({}), createComment: () => ({}),
    insert() {}, remove() {}, patchProp() {}, setText() {}, setElementText() {},
    parentNode: () => null, nextSibling: () => null,
  });
  app = renderer.createApp({ setup(props, context) {
    form = component.setup(reactive({session: {id:'session', protocol:'ssh', home:'/home/test', connected:true}, visible:true, fullscreen:false}), context);
    return () => null;
  }});
  app.provide(ssrContextKey, { modules: new Set() });
  app.mount({});
  await nextTick();
  form.localPanel.value = {path:'/local', separator:'/', refresh:async () => {}};
  form.remotePanel.value = {path:'/remote', refresh:async () => {}};
  form.showLocal.value = true;
  const folder = {name:'folder', path:'/source/folder', isDir:true, isLink:false, size:0};
  const report = {directory:true, files:2, directories:3, bytes:42, skipped:1, skippedPaths:['link']};
  const progress = {planning:false, directory:true, bytes:20, totalBytes:42, files:1, totalFiles:2, currentPath:'/source/folder/file'};
  await form.transfer('local', [folder]);
  let pending = transport.pending;
  assert.equal(pending.args.local, '/source/folder');
  assert.equal(pending.args.remotePath, '/remote/folder');
  assert.equal(pending.args.overwrite, false);
  pending.args.output.onmessage(progress);
  assert.equal(form.jobs.value[0].progress.bytes, 20);
  pending.resolve(report);
  await settle();
  assert.equal(form.jobs.value[0].status, 'done');
  assert.equal(form.jobs.value[0].report.skipped, 1);
  assert.equal(form.queueOpen.value, true, 'Skipped links must be visible');

  await form.transfer('remote', [folder]);
  transport.pending.reject('Target exists');
  await settle();
  const directoryJob = form.jobs.value.at(-1);
  const calls = transport.calls.length;
  form.replace.value = directoryJob;
  form.retryReplace();
  assert.equal(transport.calls.length, calls, 'Folder errors must never enable overwrite retry');
  assert.equal(directoryJob.overwrite, false);

  const file = {...folder, isDir:false, name:'file.txt', path:'/source/file.txt'};
  await form.transfer('remote', [file]);
  transport.pending.reject('File exists');
  await settle();
  const fileJob = form.jobs.value.at(-1);
  const oldId = fileJob.id;
  form.replace.value = fileJob;
  form.retryReplace();
  pending = transport.pending;
  assert.equal(pending.args.overwrite, true);
  assert.notEqual(pending.args.transferId, oldId, 'A retry needs a new native registration');
  pending.resolve({...report, directory:false, skipped:0});
  await settle();

  form.showLocal.value = false;
  await form.transfer('remote', [folder]);
  assert.equal(transport.dialogs.at(-1).directory, true);
  assert.equal(transport.pending.args.local, '/downloads/folder');
  const cancelled = form.jobs.value.at(-1);
  await form.cancelJob(cancelled);
  await settle();
  assert.equal(cancelled.status, 'cancelled');
  assert.equal(transport.calls.at(-1).args.transferId, cancelled.id);

  await form.upload(['C:\\Users\\test\\nested-folder']);
  assert.equal(form.jobs.value.at(-1).name, 'nested-folder');
  assert.equal(transport.pending.args.remotePath, '/remote/nested-folder');
  app.unmount();
  app = null;
  await settle();
  assert.equal(transport.calls.at(-1).name, 'remote_transfer_cancel', 'Closing the file workspace cancels its active job');
  console.log('File transfers: folder routing, progress/skips, safe retries, cancellation, Windows paths and lifecycle verified.');
} finally {
  app?.unmount();
  globalThis.window = originalWindow;
  await server.close();
  await rm(cacheDir, {recursive:true, force:true});
}
