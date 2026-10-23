<script setup lang="ts">
import { computed, ref, watch } from 'vue';
import type { CredentialFolder, CredentialInfo } from '../types';
import { action, notify, store } from '../composables/useNoryn';
import { showMenu } from '../composables/useContextMenu';
import {
  credentialFolderDescendants,
  credentialFolderOptions,
  credentialFolderPath,
  deleteCredentialFolder,
  moveCredential,
  saveCredentialFolder,
} from '../composables/useCredentialFolders';
import Icon from './Icon.vue';
import Modal from './Modal.vue';
import SelectMenu from './SelectMenu.vue';

const props = defineProps<{ credentials: CredentialInfo[] }>();
const emit = defineEmits<{
  edit: [credential?: CredentialInfo];
  remove: [id: string];
  navigate: [id: string | null];
}>();
const folderId = ref<string | null>(null);
const unfiled = ref(false);
const busy = ref(false);
const folderEditor = ref<CredentialFolder | null>(null);
const removingFolder = ref<CredentialFolder | null>(null);
const moving = ref<CredentialInfo | null>(null);
const destination = ref<string | null>(null);
const crumbs = computed(() => {
  const path: CredentialFolder[] = [];
  const seen = new Set<string>();
  let folder = store.view.credentialFolders.find((entry) => entry.id === folderId.value);
  while (folder && !seen.has(folder.id)) {
    seen.add(folder.id);
    path.unshift(folder);
    folder = store.view.credentialFolders.find((entry) => entry.id === folder?.parentId);
  }
  return path;
});
const children = computed(() =>
  unfiled.value
    ? []
    : store.view.credentialFolders
        .filter((folder) => folder.parentId === folderId.value)
        .sort((a, b) => a.name.localeCompare(b.name, 'de')),
);
const accounts = computed(() =>
  props.credentials.filter((credential) => {
    const location = store.view.credentialLocations[credential.id];
    return unfiled.value ? !location : folderId.value === null || location === folderId.value;
  }),
);
const counts = computed(() => {
  const result: Record<string, number> = {};
  for (const credential of props.credentials) {
    let id = store.view.credentialLocations[credential.id];
    const seen = new Set<string>();
    while (id && !seen.has(id)) {
      seen.add(id);
      result[id] = (result[id] || 0) + 1;
      id = store.view.credentialFolders.find((folder) => folder.id === id)?.parentId || '';
    }
  }
  return result;
});
const parentOptions = computed(() => {
  const blocked = folderEditor.value
    ? credentialFolderDescendants(folderEditor.value.id)
    : new Set<string>();
  return [
    { value: null as string | null, label: 'Hauptebene' },
    ...credentialFolderOptions.value.filter((option) => option.value && !blocked.has(option.value)),
  ];
});

function navigate(id: string | null, onlyUnfiled = false) {
  folderId.value = id;
  unfiled.value = onlyUnfiled;
  emit('navigate', id);
}
watch(
  () => store.view.credentialFolders,
  () => {
    if (
      folderId.value &&
      !store.view.credentialFolders.some((folder) => folder.id === folderId.value)
    )
      navigate(null);
  },
);
function newFolder(parentId = folderId.value) {
  folderEditor.value = { id: crypto.randomUUID(), parentId, name: '' };
}
function folderMenu(event: MouseEvent, folder: CredentialFolder) {
  showMenu(event, [
    { label: 'Ordner öffnen', icon: 'FolderOpen', action: () => navigate(folder.id) },
    { label: 'Unterordner anlegen', icon: 'FolderPlus', action: () => newFolder(folder.id) },
    {
      label: 'Ordner bearbeiten',
      icon: 'Pencil',
      action: () => (folderEditor.value = { ...folder }),
    },
    {
      label: 'Ordner entfernen',
      icon: 'Trash2',
      danger: true,
      separator: true,
      action: () => (removingFolder.value = folder),
    },
  ]);
}
function move(credential: CredentialInfo) {
  moving.value = credential;
  destination.value = store.view.credentialLocations[credential.id] || null;
}
function credentialMenu(event: MouseEvent, credential: CredentialInfo) {
  showMenu(event, [
    { label: 'Zugang bearbeiten', icon: 'Pencil', action: () => emit('edit', credential) },
    { label: 'In Ordner verschieben', icon: 'Folder', action: () => move(credential) },
    {
      label: 'Zugangsdaten entfernen',
      icon: 'Trash2',
      danger: true,
      separator: true,
      action: () => emit('remove', credential.id),
    },
  ]);
}
async function saveFolder() {
  if (!folderEditor.value || busy.value) return;
  busy.value = true;
  await action(async () => {
    await saveCredentialFolder({ ...folderEditor.value!, name: folderEditor.value!.name.trim() });
    folderEditor.value = null;
    notify('Persönlicher Ordner gespeichert');
  });
  busy.value = false;
}
async function removeFolder() {
  if (!removingFolder.value || busy.value) return;
  busy.value = true;
  await action(async () => {
    const folder = removingFolder.value!;
    const wasOpen = folderId.value === folder.id;
    await deleteCredentialFolder(folder.id);
    if (wasOpen) navigate(folder.parentId);
    removingFolder.value = null;
    notify('Ordner entfernt, Zugangsdaten behalten');
  });
  busy.value = false;
}
async function saveMove() {
  if (!moving.value || busy.value) return;
  busy.value = true;
  await action(async () => {
    await moveCredential(moving.value!.id, destination.value);
    moving.value = null;
    notify('Persönliche Ablage geändert');
  });
  busy.value = false;
}
</script>
<template>
  <div class="credential-organization">
    <div class="credential-folder-toolbar">
      <nav class="credential-crumbs" aria-label="Zugangsdaten-Ordner">
        <button :aria-current="!folderId && !unfiled ? 'page' : undefined" @click="navigate(null)">
          <Icon name="KeyRound" :size="16" />Alle Zugangsdaten
        </button>
        <template v-for="folder in crumbs" :key="folder.id">
          <Icon name="ChevronRight" :size="13" />
          <button
            :aria-current="folder.id === folderId ? 'page' : undefined"
            @click="navigate(folder.id)"
          >
            {{ folder.name }}
          </button>
        </template>
        <span v-if="unfiled">/ Ohne Ordner</span>
      </nav>
      <button
        class="button secondary"
        :class="{ selected: unfiled }"
        @click="navigate(null, !unfiled)"
      >
        Ohne Ordner
      </button>
      <button class="button secondary" @click="newFolder()">
        <Icon name="FolderPlus" :size="16" />Neuer Ordner
      </button>
      <button
        v-if="folderId"
        class="icon-button"
        aria-label="Aktuellen Zugangsdaten-Ordner bearbeiten"
        @click="folderEditor = { ...crumbs[crumbs.length - 1]! }"
      >
        <Icon name="Pencil" :size="16" />
      </button>
    </div>
    <p class="credential-organization-hint">
      Deine persönliche Ablage · Ordner ändern weder Freigaben noch Verbindungszuordnungen.
    </p>
    <div v-if="children.length" class="credential-folder-grid">
      <article
        v-for="folder in children"
        :key="folder.id"
        class="credential-folder-card"
        @contextmenu="folderMenu($event, folder)"
      >
        <button class="credential-folder-open" @click="navigate(folder.id)">
          <Icon name="Folder" :size="24" />
          <span
            ><strong>{{ folder.name }}</strong
            ><small
              >{{ counts[folder.id] || 0 }}
              {{ counts[folder.id] === 1 ? 'Konto' : 'Konten' }}</small
            ></span
          >
        </button>
        <button
          class="icon-button"
          :aria-label="`${folder.name}: Ordneraktionen`"
          @click="folderMenu($event, folder)"
        >
          <Icon name="MoreHorizontal" :size="17" />
        </button>
      </article>
    </div>
    <div class="credential-grid">
      <article
        v-for="credential in accounts"
        :key="credential.id"
        class="credential-card"
        @contextmenu="credentialMenu($event, credential)"
      >
        <div class="card-icon"><Icon name="KeyRound" :size="21" /></div>
        <div>
          <h3>{{ credential.name }}</h3>
          <p class="mono">{{ credential.username }}</p>
          <button
            v-if="store.view.credentialLocations[credential.id]"
            class="credential-location"
            @click="navigate(store.view.credentialLocations[credential.id]!)"
          >
            <Icon name="Folder" :size="13" />{{
              credentialFolderPath(store.view.credentialLocations[credential.id]!)
            }}
          </button>
          <span class="tag"
            ><Icon :name="credential.teamId ? 'Users' : 'LockKeyhole'" :size="10" />{{
              credential.teamId
                ? store.view.teams.find((team) => team.id === credential.teamId)?.name ||
                  'Team-Tresor'
                : 'Privat'
            }}</span
          >
          <span v-if="credential.hasPassword && credential.kind !== 'password'" class="tag"
            >+ Passwort</span
          >
          <span class="tag">{{
            credential.kind === 'key'
              ? 'SSH-Schlüssel'
              : credential.kind === 'agent'
                ? 'SSH-Agent'
                : 'Passwort'
          }}</span>
        </div>
        <button
          class="icon-button"
          :aria-label="`${credential.name}: Aktionen`"
          @click="credentialMenu($event, credential)"
        >
          <Icon name="MoreHorizontal" :size="17" />
        </button>
      </article>
    </div>
    <div v-if="!accounts.length && !children.length" class="empty-state">
      <Icon name="KeyRound" :size="38" />
      <h3>Hier sind noch keine Zugangsdaten</h3>
      <p>Lege einen Zugang an oder verschiebe ein vorhandenes Konto in diesen Ordner.</p>
      <button class="button secondary" @click="emit('edit')">Zugang hinzufügen</button>
    </div>
  </div>
  <Modal
    v-if="folderEditor"
    :title="
      store.view.credentialFolders.some((folder) => folder.id === folderEditor?.id)
        ? 'Ordner bearbeiten'
        : 'Neuer Zugangsdaten-Ordner'
    "
    subtitle="Nur für deine Übersicht. Die Ablage bleibt in deinem verschlüsselten Tresor."
    @close="!busy && (folderEditor = null)"
  >
    <form class="form-body" @submit.prevent="saveFolder">
      <label
        >Name<input v-model="folderEditor.name" required maxlength="512" autofocus :disabled="busy"
      /></label>
      <label
        >Übergeordneter Ordner<SelectMenu
          v-model="folderEditor.parentId"
          label="Übergeordneter Zugangsdaten-Ordner"
          :options="parentOptions"
          :disabled="busy"
      /></label>
      <div class="modal-actions">
        <button
          type="button"
          class="button secondary"
          :disabled="busy"
          @click="folderEditor = null"
        >
          Abbrechen</button
        ><button class="button primary" :disabled="busy || !folderEditor.name.trim()">
          Ordner speichern
        </button>
      </div>
    </form>
  </Modal>
  <Modal
    v-if="removingFolder"
    title="Ordner entfernen?"
    :subtitle="removingFolder.name"
    @close="!busy && (removingFolder = null)"
  >
    <div class="form-body">
      <p>
        Zugangsdaten und Unterordner bleiben erhalten und werden eine Ebene höher abgelegt.
        Verbindungen und Freigaben bleiben bestehen.
      </p>
      <div class="modal-actions">
        <button class="button secondary" :disabled="busy" @click="removingFolder = null">
          Abbrechen</button
        ><button class="button danger" :disabled="busy" @click="removeFolder">
          Ordner entfernen
        </button>
      </div>
    </div>
  </Modal>
  <Modal
    v-if="moving"
    title="Zugangsdaten ablegen"
    :subtitle="moving.name"
    @close="!busy && (moving = null)"
  >
    <form class="form-body" @submit.prevent="saveMove">
      <label
        >Ordner<SelectMenu
          v-model="destination"
          label="Zielordner für Zugangsdaten"
          :options="credentialFolderOptions"
          :disabled="busy"
      /></label>
      <p>Die Ablage ändert weder das Konto noch seine Freigabe oder Verbindungszuordnung.</p>
      <div class="modal-actions">
        <button type="button" class="button secondary" :disabled="busy" @click="moving = null">
          Abbrechen</button
        ><button class="button primary" :disabled="busy">Verschieben</button>
      </div>
    </form>
  </Modal>
</template>
