import assert from 'node:assert/strict';
import { fileURLToPath } from 'node:url';
import { createServer } from 'vite';
import { createRenderer, nextTick, ssrContextKey } from 'vue';
import { mkdtemp, rm, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';

// Real form/store lifecycle with in-memory IPC; never touch a user's vault.
const cacheDir = await mkdtemp(join(tmpdir(), 'noryn-vault-reset-'));
const mockPath = join(cacheDir, 'ipc.mjs');
await writeFile(mockPath, `
  export const transport = { calls: [], pending: null };
  export const isTauri = () => true;
  export class Channel {}
  export async function invoke(name, args) {
    transport.calls.push({name, args});
    if (name === 'vault_reset_preview') return {id:'reset-preview'};
    if (name === 'vault_reset_cancel') return;
    if (name === 'vault_reset_apply') return new Promise((resolve, reject) => transport.pending = {resolve, reject});
    throw new Error('Unexpected IPC: '+name);
  }
`);
const server = await createServer({cacheDir, optimizeDeps:{noDiscovery:true, include:[]}, resolve:{alias:{'@tauri-apps/api/core':mockPath}}, root:fileURLToPath(new URL('..',import.meta.url)), server:{middlewareMode:true,hmr:false,watch:null}});
let app;
try {
  const { transport } = await server.ssrLoadModule(mockPath);
  const { store } = await server.ssrLoadModule('/src/composables/useNoryn.ts');
  const initial = JSON.parse(JSON.stringify(store.view));
  store.view = {...initial, localRevision:500, nodes:[{id:'old-node'}]};
  const component = (await server.ssrLoadModule('/src/components/VaultReset.vue')).default;
  let form;
  const renderer = createRenderer({createElement:()=>({}),createText:()=>({}),createComment:()=>({}),insert(){},remove(){},patchProp(){},setText(){},setElementText(){},parentNode:()=>null,nextSibling:()=>null});
  app = renderer.createApp({setup(props,context){form=component.setup(props,context);return ()=>null;}});
  app.provide(ssrContextKey, {modules:new Set()});
  app.mount({});
  await nextTick();
  assert.equal(form.preview.value.id,'reset-preview');
  await form.submit();
  assert.match(form.error.value,/ZURÜCKSETZEN/);
  assert.equal(transport.calls.length,1);
  form.confirmation.value='ZURÜCKSETZEN';
  form.password.value='asdf';
  form.repeat.value='asdf';
  await form.submit();
  assert.match(form.error.value,/mindestens 12/);
  assert.equal(transport.calls.length,1);
  form.password.value='new-test-password-123';
  form.repeat.value='different-test-password';
  await form.submit();
  assert.match(form.error.value,/nicht überein/);
  assert.equal(transport.calls.length,1);
  form.repeat.value=form.password.value;
  const failure=form.submit();
  transport.pending.reject('Stale confirmation');
  await failure;
  assert.match(form.error.value,/Stale confirmation/);
  assert.equal(store.unlocked,false);
  assert.equal(store.view.localRevision,500);
  assert.equal(form.password.value,'');
  assert.equal(form.repeat.value,'');
  form.password.value='new-test-password-456';
  form.repeat.value=form.password.value;
  store.workspace='old-team';store.folder='old-folder';store.selected='old-node';store.query='old-query';store.protocol='ssh';store.favorites=true;
  const reset=form.submit();
  assert.equal(form.busy.value,true);
  assert.equal(store.loading,true);
  assert.equal(transport.calls.at(-1).args.confirmation,'ZURÜCKSETZEN');
  transport.pending.resolve({...initial,localRevision:501});
  await reset;
  assert.equal(store.unlocked,true);
  assert.equal(store.exists,true);
  assert.equal(store.workspace,'private');
  assert.equal(store.folder,'all');
  assert.equal(store.selected,'');
  assert.equal(store.query,'');
  assert.equal(store.protocol,'all');
  assert.equal(store.favorites,false);
  assert.equal(store.loading,false);
  assert.equal(form.password.value,'');
  store.view={...initial,localRevision:500,nodes:[{id:'late-old-reply'}]};
  assert.equal(store.view.localRevision,501);
  assert.equal(store.view.nodes.length,0,'Late IPC must not restore the removed tree');
  console.log('Vault reset: explicit confirmation, password validation/clearing, failures, fresh state and stale reply protection verified.');
} finally {
  app?.unmount();
  await server.close();
  await rm(cacheDir,{recursive:true,force:true});
}
