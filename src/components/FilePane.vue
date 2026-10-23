<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref } from 'vue';
import { invoke } from '@tauri-apps/api/core';
import { open, save } from '@tauri-apps/plugin-dialog';
import { getCurrentWebview } from '@tauri-apps/api/webview';
import type { ClientSession, RemoteEntry } from '../types';
import { action, desktop, notify, store } from '../composables/useHarbor';
import Icon from './Icon.vue';
import Modal from './Modal.vue';
const props = defineProps<{ session: ClientSession; visible: boolean }>();
const path = ref(props.session.home);
const pathInput = ref(path.value);
const entries = ref<RemoteEntry[]>([]);
const busy = ref(false);
const transfer = ref('');
const sudo = ref(false);
const sudoDialog = ref(false);
const sudoPassword = ref('');
const mkdirDialog = ref(false);
const newFolder = ref('');
const hidden = ref(false);
const dragging = ref(false);
const fileError = ref('');
let unlisten: (() => void) | undefined;
let disposed = false;
let request = 0;
const visibleEntries = computed(() =>
  entries.value.filter((e) => hidden.value || !e.name.startsWith('.')),
);
function fileSize(size: number) {
  return size < 1024
    ? `${size} B`
    : size < 1024 * 1024
      ? `${(size / 1024).toFixed(1)} KB`
      : `${(size / 1024 / 1024).toFixed(1)} MB`;
}
function join(name: string) {
  return `${path.value.replace(/\/$/, '')}/${name}`;
}
async function load(target = path.value) {
  const number = ++request;
  busy.value = true;
  fileError.value = '';
  try {
    const result = desktop
      ? await invoke<RemoteEntry[]>('remote_list', { id: props.session.id, path: target })
      : [
          'backups',
          'config',
          'deploy',
          '.ssh',
          'README.md',
          'docker-compose.yml',
          'access.log',
        ].map((name, i) => ({
          name,
          path: `${target.replace(/\/$/, '')}/${name}`,
          isDir: i < 4,
          isLink: false,
          size: i < 4 ? 4096 : [842, 1840, 24682][i - 4],
          modified: 0,
          permissions: i < 4 ? '755' : '644',
        }));
    if (request === number) {
      entries.value = result;
      path.value = target;
      pathInput.value = target;
    }
  } catch (e) {
    if (request === number) {
      fileError.value = String(e);
      pathInput.value = path.value;
    }
  } finally {
    if (request === number) busy.value = false;
  }
}
async function upload(paths?: string[]) {
  if (!desktop) {
    notify('Drag & Drop und Dateitransfers sind in der Desktop-App verfügbar.');
    return;
  }
  if (transfer.value) return;
  if (!paths) {
    const selected = await open({ multiple: true, directory: false, title: 'Dateien hochladen' });
    if (!selected) return;
    paths = Array.isArray(selected) ? selected : [selected];
  }
  const destination = path.value;
  await action(async () => {
    try {
      for (const [index, local] of paths!.entries()) {
        const name = local.split(/[\\/]/).pop()!;
        transfer.value = `Upload ${index + 1}/${paths!.length} · ${name}`;
        await invoke('remote_transfer', {
          id: props.session.id,
          direction: 'upload',
          local,
          remotePath: `${destination.replace(/\/$/, '')}/${name}`,
        });
      }
      notify(`${paths!.length} Datei(en) hochgeladen`);
    } finally {
      transfer.value = '';
      await load();
    }
  });
}
async function download(entry: RemoteEntry) {
  if (transfer.value || !props.session.connected) return;
  if (!desktop) {
    notify('Dateidownload ist in der Desktop-App verfügbar.');
    return;
  }
  const local = await save({ defaultPath: entry.name, title: 'Datei herunterladen' });
  if (!local) return;
  await action(async () => {
    transfer.value = `Download · ${entry.name}`;
    try {
      await invoke('remote_transfer', {
        id: props.session.id,
        direction: 'download',
        local,
        remotePath: entry.path,
      });
      notify('Datei heruntergeladen');
    } finally {
      transfer.value = '';
    }
  });
}
async function toggleSudo() {
  if (!desktop) {
    sudoDialog.value = false;
    sudoPassword.value = '';
    notify('sudo-Dateizugriff ist in der Desktop-App verfügbar.');
    return;
  }
  busy.value = true;
  await action(async () => {
    try {
      await invoke('remote_sudo', {
        id: props.session.id,
        enabled: !sudo.value,
        password: sudoPassword.value,
      });
      sudo.value = !sudo.value;
      sudoDialog.value = false;
      await load();
    } finally {
      sudoPassword.value = '';
    }
  });
  busy.value = false;
}
async function mkdir() {
  if (!newFolder.value || /[\/\0]/.test(newFolder.value) || ['.', '..'].includes(newFolder.value))
    return;
  if (!desktop) {
    notify('Ordner werden in der Desktop-App angelegt.');
    mkdirDialog.value = false;
    return;
  }
  await action(async () => {
    await invoke('remote_mkdir', { id: props.session.id, path: join(newFolder.value) });
    mkdirDialog.value = false;
    newFolder.value = '';
    await load();
  });
}
onMounted(async () => {
  void load();
  if (desktop) {
    const cleanup = await getCurrentWebview().onDragDropEvent((event) => {
      if (store.activeTab !== props.session.id || !props.visible) return;
      if (event.payload.type === 'enter') dragging.value = true;
      if (event.payload.type === 'leave') dragging.value = false;
      if (event.payload.type === 'drop') {
        dragging.value = false;
        void upload(event.payload.paths);
      }
    });
    if (disposed) cleanup();
    else unlisten = cleanup;
  }
});
onBeforeUnmount(() => {
  disposed = true;
  unlisten?.();
});
</script>
<template>
  <div class="file-pane" :class="{ 'drop-active': dragging }">
    <div class="file-heading">
      <span
        ><Icon name="Files" :size="16" />Dateien
        <span class="small-badge">{{ sudo ? 'sudo / SSH' : 'SFTP / SSH' }}</span></span
      >
      <div>
        <button
          class="icon-button"
          title="Versteckte Dateien"
          :class="{ selected: hidden }"
          @click="hidden = !hidden"
        >
          <Icon name="Eye" :size="15" /></button
        ><button class="icon-button" title="Aktualisieren" :disabled="busy" @click="load()">
          <Icon name="RefreshCw" :size="15" :class="{ spin: busy }" />
        </button>
      </div>
    </div>
    <form class="file-path" @submit.prevent="load(pathInput)">
      <button
        type="button"
        class="icon-button"
        title="Übergeordneter Ordner"
        :disabled="busy"
        @click="load(path.replace(/\/?[^/]+\/?$/, '') || '/')"
      >
        <Icon name="ArrowUp" :size="15" /></button
      ><input v-model="pathInput" class="mono" aria-label="Remote-Pfad" /><button
        class="icon-button"
        title="Pfad öffnen"
      >
        <Icon name="ArrowRight" :size="14" />
      </button>
    </form>
    <div class="file-actions">
      <button
        class="button secondary small"
        :disabled="!!transfer || !session.connected"
        @click="upload()"
      >
        <Icon name="Upload" :size="13" />Upload</button
      ><button
        class="icon-button"
        title="Neuer Ordner"
        :disabled="!!transfer || !session.connected"
        @click="mkdirDialog = true"
      >
        <Icon name="FolderPlus" :size="16" /></button
      ><button
        class="sudo-button"
        :class="{ enabled: sudo }"
        :disabled="busy || !!transfer || !session.connected"
        @click="sudo ? toggleSudo() : (sudoDialog = true)"
      >
        <Icon name="ShieldCheck" :size="13" />{{ sudo ? 'root aktiv' : 'sudo-Modus' }}
      </button>
    </div>
    <div class="file-list">
      <div class="file-columns"><span>Name</span><span>Größe</span></div>
      <p v-if="fileError" class="inline-error">{{ fileError }}</p>
      <div
        v-for="entry in visibleEntries"
        :key="entry.path"
        class="file-row"
        @dblclick="entry.isDir ? load(entry.path) : download(entry)"
      >
        <Icon
          :name="entry.isDir ? 'Folder' : 'Files'"
          :size="16"
          :class="{ 'folder-icon': entry.isDir }"
        /><button
          class="file-name ellipsis"
          @click="entry.isDir ? load(entry.path) : download(entry)"
        >
          {{ entry.name }}{{ entry.isLink ? ' ↗' : '' }}</button
        ><span class="file-size mono">{{ entry.isDir ? '—' : fileSize(entry.size) }}</span
        ><button
          v-if="!entry.isDir"
          class="icon-button"
          title="Herunterladen"
          :disabled="!!transfer || !session.connected"
          @click="download(entry)"
        >
          <Icon name="Download" :size="13" /></button
        ><span v-else class="file-download-spacer"></span>
      </div>
      <div v-if="!busy && !entries.length && !fileError" class="file-empty">
        Dieser Ordner ist leer.
      </div>
    </div>
    <div class="file-footer">
      <Icon
        :name="transfer ? 'LoaderCircle' : 'Upload'"
        :size="14"
        :class="{ spin: transfer }"
      /><span>{{
        transfer ||
        (session.preview
          ? 'Beispieldateien · Browser-Vorschau'
          : 'Dateien hierher ziehen zum Hochladen')
      }}</span>
    </div>
    <div v-if="dragging" class="drop-overlay">
      <Icon name="Upload" :size="30" /><strong>Hier hochladen</strong><span>{{ path }}</span>
    </div>
    <Modal
      v-if="sudoDialog"
      title="Dateizugriff mit sudo"
      subtitle="Öffnet einen zusätzlichen Dateizugriff als root über dieselbe SSH-Verbindung. Auf dem Zielsystem werden Python 3 und sudo-Rechte benötigt. Das Terminal bleibt unverändert."
      @close="
        sudoDialog = false;
        sudoPassword = '';
      "
      ><form class="form-body" @submit.prevent="toggleSudo">
        <label
          >sudo-Passwort<input
            v-model="sudoPassword"
            type="password"
            autocomplete="off"
            placeholder="Leer lassen bei passwortlosem sudo"
            autofocus
        /></label>
        <p class="muted">
          Das Passwort wird nur für diese Verbindung im Arbeitsspeicher gehalten und nicht
          gespeichert.
        </p>
        <div class="modal-actions">
          <button
            type="button"
            class="button secondary"
            @click="
              sudoDialog = false;
              sudoPassword = '';
            "
          >
            Abbrechen</button
          ><button class="button primary" :disabled="busy">sudo aktivieren</button>
        </div>
      </form></Modal
    >
    <Modal
      v-if="mkdirDialog"
      title="Neuer Remote-Ordner"
      :subtitle="path"
      @close="mkdirDialog = false"
      ><form class="form-body" @submit.prevent="mkdir">
        <label>Ordnername<input v-model="newFolder" required autofocus pattern="[^/]+" /></label>
        <div class="modal-actions"><button class="button primary">Ordner anlegen</button></div>
      </form></Modal
    >
  </div>
</template>
