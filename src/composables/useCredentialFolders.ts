import { computed } from 'vue';
import { invoke } from '@tauri-apps/api/core';
import type { CredentialFolder } from '../types';
import { desktop, store } from './useNoryn';

export function credentialFolderPath(id: string) {
  const names: string[] = [];
  const seen = new Set<string>();
  let folder = store.view.credentialFolders.find((folder) => folder.id === id);
  while (folder && !seen.has(folder.id)) {
    seen.add(folder.id);
    names.unshift(folder.name);
    folder = store.view.credentialFolders.find((parent) => parent.id === folder?.parentId);
  }
  return names.join(' / ');
}

export function credentialFolderDescendants(id: string) {
  const ids = new Set([id]);
  let changed = true;
  while (changed) {
    changed = false;
    for (const folder of store.view.credentialFolders) {
      if (folder.parentId && ids.has(folder.parentId) && !ids.has(folder.id)) {
        ids.add(folder.id);
        changed = true;
      }
    }
  }
  return ids;
}

export const credentialFolderOptions = computed(() => [
  { value: null as string | null, label: 'Ohne Ordner' },
  ...store.view.credentialFolders
    .map((folder) => ({
      value: folder.id as string | null,
      label: credentialFolderPath(folder.id),
    }))
    .sort((a, b) => a.label.localeCompare(b.label, 'de')),
]);

export async function saveCredentialFolder(folder: CredentialFolder) {
  if (desktop) store.view = await invoke('credential_folder_save', { folder });
  else
    store.view.credentialFolders = [
      ...store.view.credentialFolders.filter((entry) => entry.id !== folder.id),
      { ...folder },
    ];
}

export async function deleteCredentialFolder(id: string) {
  if (desktop) store.view = await invoke('credential_folder_delete', { id });
  else {
    const parent =
      store.view.credentialFolders.find((folder) => folder.id === id)?.parentId || null;
    store.view.credentialFolders = store.view.credentialFolders
      .filter((folder) => folder.id !== id)
      .map((folder) => (folder.parentId === id ? { ...folder, parentId: parent } : folder));
    for (const credential in store.view.credentialLocations) {
      if (store.view.credentialLocations[credential] !== id) continue;
      if (parent) store.view.credentialLocations[credential] = parent;
      else delete store.view.credentialLocations[credential];
    }
  }
}

export async function moveCredential(credentialId: string, folderId: string | null) {
  if (desktop) store.view = await invoke('credential_move', { credentialId, folderId });
  else if (folderId) store.view.credentialLocations[credentialId] = folderId;
  else delete store.view.credentialLocations[credentialId];
}
