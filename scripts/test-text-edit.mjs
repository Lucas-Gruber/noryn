import assert from 'node:assert/strict';
import { fileURLToPath } from 'node:url';
import { createServer } from 'vite';
import { createRenderer, nextTick, reactive, ssrContextKey } from 'vue';
import { mkdtemp, rm, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
const cacheDir = await mkdtemp(join(tmpdir(), 'noryn-text-ui-'));
const mockPath = join(cacheDir, 'ipc.mjs');
await writeFile(
  mockPath,
  `
 export const transport={calls:[],conflict:false,pending:null,wait:null,automatic:false};
 export const isTauri=()=>true;
 export class Channel {}
 export async function invoke(name,args) {
   transport.calls.push({name,args});
   if(name==='text_open') return transport.pending ? await transport.pending : {id:'doc',path:args.path,name:'config.yml',remote:args.side==='remote',textCopy:false,autoFinish:transport.automatic,editorNote:'test'};
   if(name==='text_wait') return await transport.wait;
   if(name==='text_finish') {if(transport.conflict) throw new Error('Original wurde inzwischen geändert');return true;}
   if(name==='text_reopen') return {id:'doc',autoFinish:transport.automatic,editorNote:'test'};
   if(name==='text_close') return;
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
let app;
try {
  const { transport } = await server.ssrLoadModule(mockPath);
  const component = (await server.ssrLoadModule('/src/components/ExternalDocuments.vue')).default;
  let form,
    saved = 0;
  const session = reactive({ id: 'session', connected: true });
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
  app = renderer.createApp({
    setup() {
      form = component.setup(
        { session },
        {
          emit(event) {
            if (event === 'saved') saved++;
          },
          expose() {},
        },
      );
      return () => null;
    },
  });
  app.provide(ssrContextKey, { modules: new Set() });
  app.mount({});
  const entry = {
    path: '/etc/config.yml',
    name: 'config.yml',
    isDir: false,
    isFile: true,
    isLink: false,
    size: 12,
  };
  await form.openFile('remote', entry);
  await nextTick();
  assert.equal(session.editedFiles, 1);
  assert.equal(transport.calls.length, 1, 'Opening must not write back automatically');
  assert.equal(transport.calls[0].name, 'text_open');
  const document = form.documents.value[0];
  await form.openFile('remote', entry);
  assert.equal(form.documents.value.length, 1, 'Reopen must reuse the existing working copy');
  await form.reopen(document);
  assert.equal(transport.calls.at(-1).name, 'text_reopen');
  transport.conflict = true;
  await form.finish(document);
  assert.match(document.error, /inzwischen/);
  assert.equal(form.documents.value.length, 1, 'Conflicting edits must remain recoverable');
  assert.equal(saved, 0);
  transport.conflict = false;
  await form.finish(document);
  assert.equal(saved, 1);
  assert.equal(
    form.documents.value.length,
    0,
    'Finishing uploads and removes the working copy in one action',
  );
  await form.openFile('remote', entry);
  form.closing.value = form.documents.value[0];
  await form.close();
  await nextTick();
  assert.equal(transport.calls.at(-1).name, 'text_close');
  assert.equal(session.editedFiles, 0);
  transport.automatic = true;
  let editorClosed;
  transport.wait = new Promise((resolve) => (editorClosed = resolve));
  await form.openFile('remote', entry);
  assert.equal(form.documents.value.length, 1);
  assert.equal(transport.calls.at(-1).name, 'text_wait');
  assert.equal(saved, 1, 'No upload before document close');
  editorClosed(true);
  await new Promise((resolve) => setTimeout(resolve, 0));
  await nextTick();
  assert.equal(saved, 2);
  assert.equal(session.editedFiles, 0);
  let failEditor;
  transport.wait = new Promise((resolve, reject) => (failEditor = reject));
  await form.openFile('remote', entry);
  failEditor(new Error('Original wurde inzwischen geändert'));
  await new Promise((resolve) => setTimeout(resolve, 0));
  const recovered = form.documents.value[0];
  assert.equal(recovered.autoFinish, false);
  assert.match(recovered.error, /Arbeitskopie bleibt erhalten/);
  assert.equal(saved, 2);
  await form.finish(recovered);
  assert.equal(saved, 3);
  assert.equal(form.documents.value.length, 0);
  let finish;
  transport.pending = new Promise((resolve) => (finish = resolve));
  const late = form.openFile('remote', entry);
  app.unmount();
  app = null;
  finish({ ...document, id: 'late' });
  await late;
  assert.equal(
    form.documents.value.length,
    0,
    'Late replies after closing the workspace must not restore UI state',
  );
  console.log(
    'External text editing: automatic finish after editor close, manual fallback, conflict recovery, cache close, metadata-only view and disposal passed.',
  );
} finally {
  app?.unmount();
  await server.close();
  await rm(cacheDir, { recursive: true, force: true });
}
