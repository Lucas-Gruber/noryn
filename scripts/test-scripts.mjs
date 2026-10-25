import assert from 'node:assert/strict';
import { fileURLToPath } from 'node:url';
import { createServer } from 'vite';
import { mkdtemp, rm, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
const cacheDir = await mkdtemp(join(tmpdir(), 'noryn-scripts-ui-'));
const mockPath = join(cacheDir, 'ipc.mjs');
await writeFile(
  mockPath,
  `export const transport={calls:[],sync:null,wait:null};export const isTauri=()=>true;export class Channel{};export async function invoke(name,args){transport.calls.push({name,args});if(name==='script_sync') return transport.sync(args);if(name==='script_edit_list')return [];if(name==='script_edit_open')return {id:'editor',teamId:args.teamId,name:'Test',autoFinish:true,editorNote:'wait'};if(name==='script_edit_wait')return transport.wait();if(name==='script_save')return transport.view;if(name==='script_get')return {...transport.entry,body:'echo safe'};throw new Error('Unexpected IPC '+name);}`,
);
const server = await createServer({
  cacheDir,
  optimizeDeps: { noDiscovery: true, include: [] },
  resolve: { alias: { '@tauri-apps/api/core': mockPath } },
  root: fileURLToPath(new URL('..', import.meta.url)),
  server: { middlewareMode: true, hmr: false, watch: null },
});
const tick = () => new Promise((r) => setTimeout(r, 0));
async function until(check) {
  for (let i = 0; i < 2000; i++) {
    if (check()) return;
    await tick();
  }
  throw new Error('Expected update missing');
}
const clone = (value) => JSON.parse(JSON.stringify(value));
try {
  const { language } = await server.ssrLoadModule('/src/i18n.ts');
  language.value = 'de';
  const { transport } = await server.ssrLoadModule(mockPath);
  const { store } = await server.ssrLoadModule('/src/composables/useNoryn.ts');
  store.view.scriptLibraries = {};
  store.view.teams = [];
  const scripts = await server.ssrLoadModule('/src/composables/useScripts.ts');
  const editors = await server.ssrLoadModule('/src/composables/useScriptEditors.ts');
  let outstanding = 0,
    peak = 0;
  const pending = [];
  transport.sync = async (args) => {
    outstanding++;
    peak = Math.max(peak, outstanding);
    await new Promise((resolve) => pending.push(resolve));
    outstanding--;
    return { view: clone(store.view), message: 'ok' };
  };
  store.unlocked = true;
  store.view.teams = ['a', 'b', 'c'].map((id) => ({
    id,
    name: id,
    memberName: 'Alice',
    serverUrl: 'https://example.test',
  }));
  await until(() => pending.length === 2);
  assert.equal(peak, 2, 'Startup must bound parallel team requests');
  pending.splice(0).forEach((done) => done());
  await until(() => pending.length === 1);
  pending.shift()();
  await until(() => !Object.values(scripts.scriptStates).some((s) => s.busy));
  const baseline = transport.calls.filter((c) => c.name === 'script_sync').length;
  await new Promise((r) => setTimeout(r, 750));
  assert.equal(
    transport.calls.filter((c) => c.name === 'script_sync').length,
    baseline,
    'No idle polling',
  );
  const entry = {
    id: 'entry',
    parentId: null,
    kind: 'script',
    name: 'Test',
    description: '',
    bytes: 9,
    fingerprint: 'fingerprint',
    updatedAt: 1,
    updatedBy: 'Alice',
  };
  store.view.scriptLibraries.a = {
    entries: [entry],
    history: [],
    dirty: true,
    suspended: true,
    originChanged: false,
  };
  await scripts.syncScripts('a');
  assert.equal(
    transport.calls.filter((c) => c.name === 'script_sync').length,
    baseline,
    'Imported scripts must wait for explicit sync',
  );
  transport.sync = async () => ({ view: clone(store.view), message: 'manual' });
  await scripts.syncScripts('a', { manual: true });
  assert.equal(transport.calls.at(-1).args.manual, true);
  store.view.scriptLibraries.a.suspended = false;
  store.view.scriptLibraries.a.originChanged = true;
  const count = transport.calls.length;
  await scripts.syncScripts('a');
  assert.equal(
    transport.calls.length,
    count,
    'Changed destinations must require explicit confirmation',
  );
  await scripts.syncScripts('a', { manual: true, adoptOrigin: true });
  assert.equal(transport.calls.at(-1).args.adoptOrigin, true);
  store.view.scriptLibraries.a.originChanged = false;
  transport.entry = entry;
  const body = await scripts.readScript('a', entry);
  assert.equal(body.body, 'echo safe');
  transport.view = clone(store.view);
  transport.view.localRevision++;
  await scripts.saveScript('a', body, 'fingerprint');
  await scripts.saveScript('a', body, 'fingerprint');
  transport.sync = async () => ({ view: clone(store.view), message: 'saved' });
  const debounced = transport.calls.filter((c) => c.name === 'script_sync').length;
  await until(() => transport.calls.filter((c) => c.name === 'script_sync').length > debounced);
  await new Promise((r) => setTimeout(r, 650));
  assert.equal(
    transport.calls.filter((c) => c.name === 'script_sync').length,
    debounced + 1,
    'Saved edits debounce once',
  );
  let editorClose;
  transport.wait = () => new Promise((resolve) => (editorClose = resolve));
  await editors.openScriptEditor('a', entry);
  assert.equal(editors.scriptEditors.length, 1);
  assert.equal(editors.scriptEditors[0].autoFinish, true);
  const editView = clone(store.view);
  editView.localRevision++;
  editorClose({ view: editView, changed: true });
  await until(() => editors.scriptEditors.length === 0);
  assert.equal(store.view.localRevision, editView.localRevision);
  let late;
  transport.sync = () => new Promise((resolve) => (late = resolve));
  const inflight = scripts.syncScripts('a', { manual: true });
  await until(() => late);
  const stale = clone(store.view);
  stale.localRevision += 100;
  store.unlocked = false;
  await tick();
  late({ view: stale, message: 'must be discarded' });
  await inflight;
  assert.notEqual(store.view.localRevision, stale.localRevision);
  assert.equal(Object.keys(scripts.scriptStates).length, 0);
  console.log(
    'Scripts: bounded startup sync, no idle polling, import/origin gates, debounced saves, external editor completion and stale replies after lock passed.',
  );
} finally {
  await server.close();
  await rm(cacheDir, { recursive: true, force: true });
}
