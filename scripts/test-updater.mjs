import assert from 'node:assert/strict';
import { fileURLToPath } from 'node:url';
import { createServer } from 'vite';
import { mkdtemp, rm, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';

// Test the real update controller against in-memory IPC, never real packages or a vault.
const cacheDir = await mkdtemp(join(tmpdir(), 'noryn-updater-test-'));
const mockPath = join(cacheDir, 'test-ipc.mjs');
await writeFile(
  mockPath,
  `
  export const transport = { calls: [], download: null, check: null };
  export const isTauri = () => true;
  export class Channel { onmessage = () => {}; }
  export async function invoke(name, args) {
    transport.calls.push(name);
    if (name === 'update_check') return new Promise(resolve => transport.check = resolve);
    if (name === 'update_download') return new Promise((resolve, reject) => transport.download = {resolve, reject, channel: args.output});
    if (name === 'update_cancel') { transport.download.reject('Download abgebrochen'); return; }
    if (name === 'vault_lock') return;
    if (name === 'update_install') throw new Error('Simulated install failure');
    if (name === 'team_events_stop') return;
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
try {
  const { transport } = await server.ssrLoadModule(mockPath);
  const { store } = await server.ssrLoadModule('/src/composables/useNoryn.ts');
  const {
    updater,
    checkUpdates,
    downloadUpdate,
    cancelUpdate,
    installUpdate,
    downloadPercent,
    automaticCheck,
  } = await server.ssrLoadModule('/src/composables/useUpdater.ts');
  updater.version = '0.1.0';
  updater.canInstall = true;
  const checking = checkUpdates();
  await checkUpdates();
  assert.equal(transport.calls.filter((c) => c === 'update_check').length, 1);
  transport.check({ version: '0.2.0', notes: 'New version' });
  await checking;
  assert.equal(updater.phase, 'available');
  const firstDownload = downloadUpdate();
  transport.download.channel.onmessage({ downloaded: 50, total: 100 });
  assert.equal(downloadPercent.value, 50);
  await cancelUpdate();
  await firstDownload;
  assert.equal(updater.phase, 'available');
  assert.equal(updater.error, '', 'User cancellation is not a failure');
  const secondDownload = downloadUpdate();
  transport.download.resolve();
  await secondDownload;
  assert.equal(updater.phase, 'ready');
  store.unlocked = true;
  store.sessions = [{ id: 'active-ssh' }];
  await installUpdate();
  assert.ok(
    !transport.calls.includes('update_install'),
    'Active SSH sessions must block installation',
  );
  store.sessions = [];
  await installUpdate();
  assert.equal(store.unlocked, false, 'The vault must be locked before installation');
  const last = transport.calls.slice(-2);
  assert.deepEqual(last, ['vault_lock', 'update_install']);
  assert.equal(updater.phase, 'ready', 'Installation failures must remain visible and retryable');
  assert.match(updater.error, /Simulated install failure/);
  updater.phase = 'current';
  updater.enabled = true;
  updater.lastChecked = Date.now();
  const priorChecks = transport.calls.filter((c) => c === 'update_check').length;
  automaticCheck();
  assert.equal(
    transport.calls.filter((c) => c === 'update_check').length,
    priorChecks,
    'Focus must not poll within six hours',
  );
  updater.lastChecked -= 6 * 60 * 60 * 1000 + 1;
  automaticCheck();
  assert.equal(transport.calls.filter((c) => c === 'update_check').length, priorChecks + 1);
  transport.check(null);
  await new Promise((resolve) => setImmediate(resolve));
  automaticCheck();
  assert.equal(
    transport.calls.filter((c) => c === 'update_check').length,
    priorChecks + 1,
    'Successful checks reset the focus interval',
  );
  console.log(
    'Updater: progress, cancellation, session guard, vault lock ordering and retry behavior verified.',
  );
} finally {
  await server.close();
  await rm(cacheDir, { recursive: true, force: true });
}
