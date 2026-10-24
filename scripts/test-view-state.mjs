import assert from 'node:assert/strict';
import { fileURLToPath } from 'node:url';
import { createServer } from 'vite';
import { mkdtemp, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';

// Exercise the actual Vue store without a browser, native vault or saved secrets.
const cacheDir = await mkdtemp(join(tmpdir(), 'noryn-view-test-'));
const server = await createServer({
  cacheDir,
  optimizeDeps: { noDiscovery: true, include: [] },
  root: fileURLToPath(new URL('..', import.meta.url)),
  server: { middlewareMode: true, hmr: false, watch: null },
});
try {
  const { store, desktop } = await server.ssrLoadModule('/src/composables/useNoryn.ts');
  assert.equal(desktop, false);
  const original = JSON.parse(JSON.stringify(store.view));
  const updated = { ...original.nodes[0], name: 'Newer concurrent response' };
  store.view = { ...original, localRevision: 2, nodes: [updated] };
  store.view = { ...original, localRevision: 1 };
  assert.equal(store.view.localRevision, 2);
  assert.equal(store.view.nodes[0].name, updated.name);
  store.view = { ...original, localRevision: 3 };
  assert.equal(store.view.localRevision, 3);
  assert.equal(store.view.nodes.length, original.nodes.length);
  console.log('Concurrent IPC views: newer local data is preserved.');
} finally {
  await server.close();
  await rm(cacheDir, { recursive: true, force: true });
}
