import { reactive, watch } from 'vue';
import { invoke } from '@tauri-apps/api/core';
import { desktop, store } from './useNoryn';
import type { ScriptEntry, ScriptInfo, VaultView } from '../types';

export const scriptStates = reactive<
  Record<string, { busy: boolean; error: string; message: string }>
>({});
const timers = new Map<string, ReturnType<typeof setTimeout>>();
const queued = new Map<string, number>();
const working = new Set<string>();
let generation = 0,
  active = 0,
  disposed = false;
const demoBodies = new Map<string, ScriptEntry>();
const stateFor = (id: string) => (scriptStates[id] ||= { busy: false, error: '', message: '' });
const current = (id: string, epoch: number) =>
  !disposed && store.unlocked && generation === epoch && store.view.teams.some((t) => t.id === id);
export async function syncScripts(
  id: string,
  options: {
    manual?: boolean;
    adoptOrigin?: boolean;
    republishMissing?: boolean;
    keepBoth?: boolean;
  } = {},
) {
  const epoch = generation;
  if (!current(id, epoch) || working.has(id)) return;
  const state = stateFor(id);
  const summary = store.view.scriptLibraries?.[id];
  if (!options.manual && (summary?.suspended || summary?.originChanged)) return;
  clearTimeout(timers.get(id));
  timers.delete(id);
  if (!desktop) {
    state.message = 'Browser-Vorschau · kein Serverabgleich.';
    return;
  }
  working.add(id);
  state.busy = true;
  state.error = '';
  state.message = 'Sammlung wird abgeglichen …';
  const before = summary?.entries.map((e) => e.fingerprint).join(',') || '';
  try {
    const result = await invoke<{ view: VaultView; message: string }>('script_sync', {
      teamId: id,
      manual: !!options.manual,
      adoptOrigin: !!options.adoptOrigin,
      republishMissing: !!options.republishMissing,
      keepBoth: !!options.keepBoth,
    });
    if (!current(id, epoch)) return;
    store.view = result.view;
    state.message = result.message;
    const after = store.view.scriptLibraries?.[id];
    if (
      after?.dirty &&
      !after.suspended &&
      after.entries.map((e) => e.fingerprint).join(',') !== before
    )
      scheduleScripts(id);
  } catch (error) {
    if (current(id, epoch)) {
      state.error = String(error);
      state.message = '';
    }
  } finally {
    working.delete(id);
    if (current(id, epoch)) state.busy = false;
    else if (store.unlocked && store.view.teams.some((t) => t.id === id)) enqueue(id);
  }
}
export function scheduleScripts(id: string) {
  clearTimeout(timers.get(id));
  timers.set(
    id,
    setTimeout(() => {
      timers.delete(id);
      enqueue(id);
    }, 600),
  );
}
function enqueue(id: string) {
  queued.set(id, generation);
  pump();
}
function pump() {
  while (active < 2 && queued.size) {
    const [id, epoch] = queued.entries().next().value!;
    queued.delete(id);
    if (!current(id, epoch)) continue;
    active++;
    void syncScripts(id).finally(() => {
      active--;
      pump();
    });
  }
}
export async function readScript(teamId: string, info: ScriptInfo): Promise<ScriptEntry> {
  if (desktop) return invoke<ScriptEntry>('script_get', { teamId, entryId: info.id });
  return {
    ...info,
    body:
      demoBodies.get(info.id)?.body ||
      (info.kind === 'script'
        ? '#!/usr/bin/env bash\nset -euo pipefail\n\nprintf "Host: %s\\n" "$(hostname)"\nuptime\n'
        : ''),
  };
}
export async function saveScript(teamId: string, entry: ScriptEntry, expected: string | null) {
  if (desktop) store.view = await invoke<VaultView>('script_save', { teamId, entry, expected });
  else {
    const summary = (store.view.scriptLibraries[teamId] ||= {
      entries: [],
      history: [],
      dirty: false,
      suspended: false,
      originChanged: false,
    });
    entry = {
      ...entry,
      updatedAt: Date.now(),
      updatedBy: store.view.teams.find((t) => t.id === teamId)?.memberName || 'Vorschau',
    };
    demoBodies.set(entry.id, { ...entry });
    summary.entries = [
      ...summary.entries.filter((e) => e.id !== entry.id),
      {
        ...entry,
        bytes: new TextEncoder().encode(entry.body).length,
        fingerprint: crypto.randomUUID(),
      },
    ];
    summary.dirty = true;
  }
  scheduleScripts(teamId);
}
export async function deleteScript(teamId: string, info: ScriptInfo) {
  if (desktop)
    store.view = await invoke<VaultView>('script_delete', {
      teamId,
      entryId: info.id,
      expected: info.fingerprint,
    });
  else {
    const summary = store.view.scriptLibraries[teamId]!;
    summary.entries = summary.entries
      .filter((e) => e.id !== info.id)
      .map((e) => (e.parentId === info.id ? { ...e, parentId: info.parentId } : e));
    demoBodies.delete(info.id);
    summary.dirty = true;
  }
  scheduleScripts(teamId);
}
const stop = watch(
  () =>
    store.unlocked
      ? store.view.teams
          .map((t) => t.id)
          .sort()
          .join('\n')
      : null,
  (ids, old) => {
    if (ids === null) {
      generation++;
      queued.clear();
      demoBodies.clear();
      for (const timer of timers.values()) clearTimeout(timer);
      timers.clear();
      for (const id of Object.keys(scriptStates)) delete scriptStates[id];
      return;
    }
    const previous = new Set((old || '').split('\n'));
    for (const id of ids.split('\n').filter(Boolean)) if (!previous.has(id)) enqueue(id);
    for (const id of Object.keys(scriptStates))
      if (!store.view.teams.some((t) => t.id === id)) {
        delete scriptStates[id];
        queued.delete(id);
        clearTimeout(timers.get(id));
        timers.delete(id);
      }
  },
  { immediate: true },
);
if (import.meta.hot)
  import.meta.hot.dispose(() => {
    disposed = true;
    stop();
    queued.clear();
    for (const timer of timers.values()) clearTimeout(timer);
  });
