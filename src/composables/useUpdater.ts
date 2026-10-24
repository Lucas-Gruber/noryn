import { computed, reactive, watch } from 'vue';
import { Channel, invoke } from '@tauri-apps/api/core';
import { desktop, lock, store } from './useNoryn';
import { liveStates } from './useLiveSync';
import packageInfo from '../../package.json';

const preferenceKey = 'noryn.updates.v1';
const interval = 6 * 60 * 60 * 1000;
let cancelRequested = false;
let automatic = true;
try {
  automatic = JSON.parse(localStorage.getItem(preferenceKey) || '{}').automatic !== false;
} catch {
  /* Use the default when storage is unavailable. */
}
export const updater = reactive({
  automatic,
  version: packageInfo.version,
  enabled: false,
  canInstall: false,
  hint: desktop
    ? 'Update-Informationen werden geladen …'
    : 'Updates sind in der Desktop-App verfügbar.',
  phase: 'idle' as
    | 'idle'
    | 'checking'
    | 'available'
    | 'downloading'
    | 'ready'
    | 'installing'
    | 'current'
    | 'error',
  offer: null as null | { version: string; notes: string },
  downloaded: 0,
  total: null as number | null,
  lastChecked: 0,
  error: '',
  dialog: false,
  dismissed: '',
});
watch(
  () => updater.automatic,
  (value) => {
    try {
      localStorage.setItem(preferenceKey, JSON.stringify({ automatic: value }));
    } catch {
      /* Session preference still applies. */
    }
    if (value) automaticCheck();
  },
);
export const updateBusy = computed(() =>
  ['checking', 'downloading', 'installing'].includes(updater.phase),
);
export const installBlocked = computed(
  () =>
    store.sessions.length > 0 ||
    store.loading ||
    !!store.authPrompt ||
    !!store.challenge ||
    Object.values(liveStates).some((s) => s.phase === 'syncing'),
);
export const downloadPercent = computed(() =>
  updater.total ? Math.min(100, Math.floor((updater.downloaded / updater.total) * 100)) : null,
);

export async function checkUpdates() {
  if (!desktop || updateBusy.value || updater.phase === 'ready') return;
  updater.phase = 'checking';
  updater.error = '';
  try {
    updater.offer = await invoke('update_check');
    updater.lastChecked = Date.now();
    updater.phase = updater.offer ? 'available' : 'current';
  } catch (error) {
    updater.error = String(error);
    updater.phase = 'error';
    updater.lastChecked = Date.now();
  }
}
export async function downloadUpdate() {
  if (!desktop || updateBusy.value || !updater.offer || !updater.canInstall) return;
  updater.phase = 'downloading';
  updater.error = '';
  cancelRequested = false;
  updater.downloaded = 0;
  updater.total = null;
  const output = new Channel<{ downloaded: number; total: number | null }>();
  output.onmessage = (progress) => {
    updater.downloaded = progress.downloaded;
    updater.total = progress.total;
  };
  try {
    await invoke('update_download', { output });
    updater.phase = 'ready';
  } catch (error) {
    updater.error = cancelRequested && String(error).includes('abgebrochen') ? '' : String(error);
    updater.phase = 'available';
  }
}
export async function cancelUpdate() {
  if (updater.phase !== 'downloading') return;
  cancelRequested = true;
  try {
    await invoke('update_cancel');
  } catch (error) {
    updater.error = String(error);
  }
}
export async function installUpdate() {
  if (updater.phase !== 'ready' || installBlocked.value) return;
  updater.phase = 'installing';
  updater.error = '';
  try {
    // The native installer independently requires a locked vault and no active sessions/jobs.
    await lock();
    await invoke('update_install');
  } catch (error) {
    updater.error = String(error);
    updater.phase = 'ready';
  }
}
export async function openReleases() {
  if (!desktop) return;
  try {
    await invoke('update_open_release');
  } catch (error) {
    updater.error = String(error);
  }
}
export async function openProject() {
  if (!desktop) {
    window.open('https://github.com/Lucas-Gruber/noryn', '_blank', 'noopener,noreferrer');
    return;
  }
  try {
    await invoke('program_open_project');
  } catch (error) {
    store.error = String(error);
  }
}
export function automaticCheck() {
  if (!updater.enabled || !updater.automatic || Date.now() - updater.lastChecked < interval) return;
  void checkUpdates();
}
let startup: ReturnType<typeof setTimeout> | undefined;
if (desktop && typeof window !== 'undefined') {
  void invoke<{ version: string; automatic: boolean; canInstall: boolean; installHint: string }>(
    'update_info',
  )
    .then((info) => {
      updater.version = info.version;
      updater.enabled = info.automatic;
      updater.canInstall = info.canInstall;
      updater.hint = info.installHint;
      startup = setTimeout(automaticCheck, 8000);
    })
    .catch((error) => {
      updater.error = String(error);
      updater.phase = 'error';
    });
  // No polling timer: check at launch and when returning after at least six hours.
  window.addEventListener('focus', automaticCheck);
}
if (import.meta.hot)
  import.meta.hot.dispose(() => {
    clearTimeout(startup);
    if (typeof window !== 'undefined') window.removeEventListener('focus', automaticCheck);
  });
