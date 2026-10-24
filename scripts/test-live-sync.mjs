import assert from 'node:assert/strict';
import { fileURLToPath } from 'node:url';
import { createServer } from 'vite';
import { mkdtemp, rm, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';

// Test actual orchestration against a local in-memory IPC transport, with no vault or network.
const cacheDir = await mkdtemp(join(tmpdir(), 'noryn-live-test-'));
const mockPath = join(cacheDir, 'test-ipc.mjs');
await writeFile(
  mockPath,
  `
  export const transport = { channels: new Map(), jobs: [] };
  export const isTauri = () => true;
  export class Channel { onmessage = () => {}; }
  export async function invoke(name, args) {
    if (name === 'team_events_stop') { transport.channels.delete(args.teamId); return; }
    if (name === 'team_events_start') { transport.channels.set(args.teamId, args.output); return; }
    if (name === 'team_auto_sync') return new Promise((resolve, reject) => transport.jobs.push({resolve, reject}));
    throw new Error('Unexpected IPC: ' + name);
  }
`,
);
const server = await createServer({
  cacheDir,
  optimizeDeps: { noDiscovery: true, include: [] },
  resolve: { alias: { '@tauri-apps/api/core': mockPath } },
  ssr: { noExternal: ['@tauri-apps/api'] },
  root: fileURLToPath(new URL('..', import.meta.url)),
  server: { middlewareMode: true, hmr: false, watch: null },
});
const pause = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
async function until(predicate) {
  const end = Date.now() + 4000;
  while (!predicate()) {
    assert.ok(Date.now() < end, 'Expected sync event did not arrive');
    await pause(10);
  }
}
let store;
try {
  const { transport } = await server.ssrLoadModule(mockPath);
  const state = await server.ssrLoadModule('/src/composables/useNoryn.ts');
  assert.equal(state.desktop, true, 'The test IPC transport must be active');
  store = state.store;
  await server.ssrLoadModule('/src/composables/useLiveSync.ts');
  const view = (localRevision, revision, dirty) => ({
    localRevision,
    nodes: [],
    credentials: [],
    credentialFolders: [],
    credentialLocations: {},
    bindings: {},
    teamActivity: {},
    teams: [
      {
        id: 'ops',
        name: 'Operations',
        memberName: 'Alice',
        serverUrl: 'https://sync.example.test',
        revision,
        dirty,
        liveSync: true,
      },
    ],
  });
  store.view = view(1, 'A', true);
  store.unlocked = true;
  await until(() => transport.channels.has('ops'));
  const channel = transport.channels.get('ops');
  channel.onmessage({ kind: 'connected', revision: null, message: 'Connected' });
  channel.onmessage({ kind: 'revision', revision: 'A', message: 'Revision' });
  await until(() => transport.jobs.length === 1);
  // A newer remote notice arrives before the previous upload is acknowledged.
  channel.onmessage({ kind: 'revision', revision: 'C', message: 'Revision' });
  transport.jobs[0].resolve({ view: view(2, 'B', false), message: 'OK', conflicts: [] });
  await until(() => transport.jobs.length === 2);
  transport.jobs[1].resolve({ view: view(3, 'C', false), message: 'OK', conflicts: [] });
  await pause(750);
  assert.equal(transport.jobs.length, 2, 'Own acknowledgements must not loop');
  store.view = view(4, 'C', false);
  await pause(750);
  assert.equal(transport.jobs.length, 2, 'Personal vault updates must not cause team requests');
  channel.onmessage({ kind: 'error', revision: null, message: 'Offline' });
  store.view = view(5, 'C', true);
  await pause(750);
  assert.equal(transport.jobs.length, 2, 'Offline edits must wait for reconnection');
  channel.onmessage({ kind: 'connected', revision: null, message: 'Connected' });
  channel.onmessage({ kind: 'revision', revision: 'C', message: 'Revision' });
  await until(() => transport.jobs.length === 3);
  transport.jobs[2].resolve({ view: view(6, 'D', false), message: 'OK', conflicts: [] });
  await pause(750);
  assert.equal(transport.jobs.length, 3);
  console.log(
    'Live sync: newer notices survive in-flight replies; no idle/echo requests; offline changes resume.',
  );
} finally {
  if (store) store.unlocked = false;
  await pause(20);
  await server.close();
  await rm(cacheDir, { recursive: true, force: true });
}
