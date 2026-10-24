import { reactive, watch } from 'vue';
import { Channel, invoke } from '@tauri-apps/api/core';
import { desktop, store } from './useNoryn';
import type { VaultView } from '../types';

export interface SyncConflict {
  object: string;
  id: string;
  name: string;
  fields: string[];
}
interface WatchEvent {
  kind: 'connected' | 'revision' | 'error';
  revision: string | null;
  message: string;
}
interface LiveState {
  phase: 'connecting' | 'listening' | 'syncing' | 'error' | 'conflict';
  message: string;
  conflicts: SyncConflict[];
}
export const liveStates = reactive<Record<string, LiveState>>({});
const epochs = reactive<Record<string, number>>({});
const streams = new Map<
  string,
  { key: string; channel: Channel<WatchEvent>; connected: boolean }
>();
const lifecycle = new Map<string, Promise<unknown>>();
const timers = new Map<string, ReturnType<typeof setTimeout>>();
const working = new Set<string>();
const attempts = new Map<string, string>();
const notices = new Map<string, string>();
let disposed = false;

function signature(id: string) {
  const team = store.view.teams.find((t) => t.id === id);
  return JSON.stringify([
    team?.revision,
    team?.dirty,
    (store.view.teamActivity[id] || []).map((c) => c.id),
    notices.get(id),
  ]);
}
function schedule(id: string) {
  clearTimeout(timers.get(id));
  timers.set(
    id,
    setTimeout(() => {
      timers.delete(id);
      void syncNow(id);
    }, 600),
  );
}
export function restartLiveTeam(id: string) {
  epochs[id] = (epochs[id] || 0) + 1;
  attempts.delete(id);
}

export async function syncNow(id: string, force = false) {
  if (force && liveStates[id]?.phase === 'error') {
    restartLiveTeam(id);
    return;
  }
  const record = streams.get(id);
  const team = store.view.teams.find((t) => t.id === id);
  if (
    !desktop ||
    disposed ||
    !store.unlocked ||
    !record ||
    !record.connected ||
    !team?.liveSync ||
    working.has(id)
  )
    return;
  const notice = notices.get(id);
  if (!force && !team.dirty && (!notice || notice === team.revision)) return;
  const stamp = signature(id);
  if (!force && attempts.get(id) === stamp) return;
  attempts.set(id, stamp);
  working.add(id);
  liveStates[id] = { phase: 'syncing', message: 'Änderungen werden abgeglichen …', conflicts: [] };
  try {
    const result = await invoke<{ view: VaultView; message: string; conflicts: SyncConflict[] }>(
      'team_auto_sync',
      { teamId: id },
    );
    if (disposed || !store.unlocked || streams.get(id) !== record) return;
    store.view = result.view;
    if (!result.conflicts.length && notices.get(id) === notice)
      notices.set(id, store.view.teams.find((t) => t.id === id)?.revision || 'none');
    liveStates[id] = {
      phase: result.conflicts.length ? 'conflict' : record.connected ? 'listening' : 'error',
      message:
        result.conflicts.length || record.connected
          ? result.message
          : 'Live-Verbindung unterbrochen. Erneuter Verbindungsversuch folgt automatisch.',
      conflicts: result.conflicts,
    };
  } catch (error) {
    if (store.unlocked && streams.get(id) === record)
      liveStates[id] = { phase: 'error', message: String(error), conflicts: [] };
  } finally {
    working.delete(id);
    if (store.unlocked && streams.has(id) && signature(id) !== attempts.get(id)) schedule(id);
  }
}

function queue(id: string, action: () => Promise<void>) {
  const previous = lifecycle.get(id) || Promise.resolve();
  const next = previous.catch(() => {}).then(action);
  lifecycle.set(id, next);
  void next.catch((error) => {
    if (streams.has(id)) liveStates[id] = { phase: 'error', message: String(error), conflicts: [] };
  });
}
const stopStreams = watch(
  () =>
    store.unlocked
      ? store.view.teams
          .filter((t) => t.liveSync && t.memberName.trim())
          .map((t) => JSON.stringify([t.id, t.serverUrl, t.memberName, epochs[t.id] || 0]))
          .join('\n')
      : '',
  () => {
    if (!desktop || disposed) return;
    const desired = new Map(
      store.unlocked
        ? store.view.teams
            .filter((t) => t.liveSync && t.memberName.trim())
            .map((t) => [
              t.id,
              JSON.stringify([t.id, t.serverUrl, t.memberName, epochs[t.id] || 0]),
            ])
        : [],
    );
    for (const id of streams.keys()) {
      if (!desired.has(id)) {
        streams.delete(id);
        attempts.delete(id);
        notices.delete(id);
        clearTimeout(timers.get(id));
        timers.delete(id);
        delete liveStates[id];
        queue(id, async () => {
          await invoke('team_events_stop', { teamId: id });
        });
      }
    }
    for (const [id, key] of desired) {
      if (streams.get(id)?.key === key) continue;
      const channel = new Channel<WatchEvent>();
      const record = { key, channel, connected: false };
      streams.set(id, record);
      liveStates[id] = {
        phase: 'connecting',
        message: 'Live-Verbindung wird hergestellt …',
        conflicts: [],
      };
      channel.onmessage = (event) => {
        if (disposed || !store.unlocked || streams.get(id) !== record) return;
        if (event.kind === 'connected') {
          record.connected = true;
          attempts.delete(id);
        }
        if (event.kind === 'error') record.connected = false;
        if (event.kind === 'revision' && event.revision) {
          notices.set(id, event.revision);
          if (liveStates[id]?.phase === 'error') attempts.delete(id);
          schedule(id);
        } else if (!working.has(id) && liveStates[id]?.phase !== 'conflict') {
          liveStates[id] = {
            phase: event.kind === 'error' ? 'error' : 'listening',
            message: event.message,
            conflicts: [],
          };
        }
      };
      queue(id, async () => {
        await invoke('team_events_stop', { teamId: id });
        if (streams.get(id) !== record || !store.unlocked) return;
        await invoke('team_events_start', { teamId: id, output: channel });
        schedule(id);
      });
    }
  },
  { immediate: true },
);
const stopChanges = watch(
  () => store.view,
  () => {
    if (desktop && store.unlocked)
      for (const team of store.view.teams) if (team.liveSync && team.dirty) schedule(team.id);
  },
);
if (import.meta.hot)
  import.meta.hot.dispose(() => {
    disposed = true;
    stopStreams();
    stopChanges();
    for (const timer of timers.values()) clearTimeout(timer);
    for (const id of streams.keys())
      void invoke('team_events_stop', { teamId: id }).catch(() => {});
  });
