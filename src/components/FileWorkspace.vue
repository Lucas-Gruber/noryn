<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref, watch } from 'vue';
import { Channel, invoke } from '@tauri-apps/api/core';
import { open, save } from '@tauri-apps/plugin-dialog';
import { getCurrentWebview } from '@tauri-apps/api/webview';
import type { ClientSession, RemoteEntry } from '../types';
import { isFileProtocol, isFtp } from '../protocols';
import { action, desktop, notify, store } from '../composables/useNoryn';
import FilePanel from './FilePanel.vue';
import Modal from './Modal.vue';
import Icon from './Icon.vue';
const props = defineProps<{ session: ClientSession; visible: boolean; fullscreen: boolean }>();
const emit = defineEmits<{ fullscreen: []; 'local-view': [visible: boolean] }>();
const root = ref<HTMLElement>();
const remotePanel = ref<InstanceType<typeof FilePanel>>();
const localPanel = ref<InstanceType<typeof FilePanel>>();
const showLocal = ref(isFileProtocol(props.session.protocol));
watch(showLocal, (value) => emit('local-view', value), { immediate: true });
const sudo = ref(false),
  sudoBusy = ref(false),
  sudoDialog = ref(false),
  sudoPassword = ref('');
const mkdir = ref<{ side: 'local' | 'remote'; parent: string } | null>(null);
const folderName = ref('');
interface TransferProgress {
  planning: boolean;
  directory: boolean;
  bytes: number;
  totalBytes: number;
  files: number;
  totalFiles: number;
  currentPath: string;
}
interface TransferReport {
  directory: boolean;
  files: number;
  directories: number;
  bytes: number;
  skipped: number;
  skippedPaths: string[];
}
interface Job {
  id: string;
  name: string;
  direction: 'upload' | 'download';
  local: string;
  remotePath: string;
  status: 'queued' | 'active' | 'done' | 'error' | 'cancelled';
  directory?: boolean;
  progress?: TransferProgress;
  report?: TransferReport;
  cancelRequested?: boolean;
  error?: string;
  overwrite?: boolean;
}
const jobs = ref<Job[]>([]);
const queueOpen = ref(false);
const running = ref(false);
const replace = ref<Job | null>(null);
const current = computed(() => jobs.value.find((job) => job.status === 'active'));
const remaining = computed(
  () => jobs.value.filter((job) => ['queued', 'active'].includes(job.status)).length,
);
const failed = computed(() => jobs.value.filter((job) => job.status === 'error').length);
type Drag = {
  side: 'local' | 'remote';
  entries: RemoteEntry[];
  startX: number;
  startY: number;
  x: number;
  y: number;
  active: boolean;
};
const drag = ref<Drag | null>(null);
const dropSide = ref('');
const nativeDrag = ref(false);
let cleanup: (() => void) | undefined;
let disposed = false;
function join(directory: string, name: string, separator = '/') {
  return directory.replace(/[\\/]$/, '') + separator + name;
}
function startDrag(event: PointerEvent, side: 'local' | 'remote', entries: RemoteEntry[]) {
  drag.value = {
    side,
    entries,
    startX: event.clientX,
    startY: event.clientY,
    x: event.clientX,
    y: event.clientY,
    active: false,
  };
}
function dropInfo(target: Element | null) {
  const panel = target?.closest<HTMLElement>('[data-file-side]');
  if (!panel || !root.value?.contains(panel)) return null;
  return {
    side: panel.dataset.fileSide as 'local' | 'remote',
    path:
      target?.closest<HTMLElement>('[data-drop-path]')?.dataset.dropPath ||
      panel.dataset.dropPath ||
      '',
  };
}
function pointerMove(event: PointerEvent) {
  if (!drag.value || !props.visible) return;
  if (!event.buttons) {
    drag.value = null;
    dropSide.value = '';
    return;
  }
  drag.value.x = event.clientX;
  drag.value.y = event.clientY;
  if (Math.hypot(event.clientX - drag.value.startX, event.clientY - drag.value.startY) > 5)
    drag.value.active = true;
  const target = dropInfo(document.elementFromPoint(event.clientX, event.clientY));
  dropSide.value = drag.value.active && target?.side !== drag.value.side ? target?.side || '' : '';
}
function pointerUp(event: PointerEvent) {
  const state = drag.value;
  drag.value = null;
  dropSide.value = '';
  if (!state?.active || !props.visible) return;
  const target = dropInfo(document.elementFromPoint(event.clientX, event.clientY));
  if (target && target.side !== state.side) void transfer(state.side, state.entries, target.path);
}
async function processQueue() {
  if (running.value) return;
  running.value = true;
  try {
    for (;;) {
      if (disposed) break;
      const job = jobs.value.find((job) => job.status === 'queued');
      if (!job) break;
      job.status = 'active';
      try {
        if (!desktop) throw new Error('Dateitransfers sind in der Desktop-App verfügbar.');
        const output = new Channel<TransferProgress>();
        output.onmessage = (progress) => {
          if (disposed || job.status !== 'active') return;
          job.progress = progress;
          if (!progress.planning) job.directory = progress.directory;
        };
        job.report = await invoke<TransferReport>('remote_transfer', {
          transferId: job.id,
          output,
          id: props.session.id,
          direction: job.direction,
          local: job.local,
          remotePath: job.remotePath,
          overwrite: job.overwrite || false,
        });
        job.status = 'done';
        job.directory = job.report.directory;
        if (job.report.skipped) queueOpen.value = true;
      } catch (error) {
        job.status = job.cancelRequested ? 'cancelled' : 'error';
        job.error = String(error);
        queueOpen.value = true;
      }
    }
    if (!disposed) {
      await remotePanel.value?.refresh();
      await localPanel.value?.refresh();
    }
  } finally {
    running.value = false;
  }
}
function basename(path: string) {
  return (
    path
      .replace(/[\\/]+$/, '')
      .split(/[\\/]/)
      .pop() || ''
  );
}
function bytes(value: number) {
  return value >= 1024 * 1024
    ? `${(value / (1024 * 1024)).toFixed(1)} MiB`
    : `${(value / 1024).toFixed(1)} KiB`;
}
function jobStatus(job: Job) {
  if (job.status === 'cancelled') return 'Abgebrochen · Bereits übertragene Teile bleiben erhalten';
  if (job.report)
    return `${job.report.files} Datei(en), ${job.report.directories} Ordner · ${bytes(job.report.bytes)}${job.report.skipped ? ` · ${job.report.skipped} Verknüpfung(en)/Spezialdatei(en) ausgelassen` : ''}`;
  if (job.cancelRequested) return 'Wird abgebrochen …';
  if (job.progress?.planning) return 'Ordnerinhalt wird geprüft …';
  if (job.progress)
    return `${job.progress.files} / ${job.progress.totalFiles} Dateien · ${bytes(job.progress.bytes)} / ${bytes(job.progress.totalBytes)}`;
  return job.status === 'active' ? 'Übertragung läuft' : 'Wartet';
}
function retryReplace() {
  const job = replace.value;
  if (!job || job.directory !== false) return;
  job.id = crypto.randomUUID();
  job.progress = undefined;
  job.cancelRequested = false;
  job.overwrite = true;
  job.error = '';
  job.status = 'queued';
  replace.value = null;
  void processQueue();
}
async function cancelJob(job: Job) {
  job.cancelRequested = true;
  try {
    await invoke('remote_transfer_cancel', { transferId: job.id });
  } catch (error) {
    notify(String(error));
  }
}
function enqueue(
  direction: 'upload' | 'download',
  local: string,
  remotePath: string,
  overwrite = false,
  directory?: boolean,
) {
  jobs.value.push({
    id: crypto.randomUUID(),
    name: direction === 'upload' ? basename(local) : basename(remotePath),
    direction,
    local,
    remotePath,
    status: 'queued',
    overwrite,
    directory,
  });
}
async function transfer(side: 'local' | 'remote', entries: RemoteEntry[], destination?: string) {
  if (!entries.length) return;
  if (!desktop) {
    notify('Die Vorschau zeigt den Ablauf. Echte Transfers laufen in der Desktop-App.');
    return;
  }
  for (const file of entries) {
    if (side === 'local')
      enqueue(
        'upload',
        file.path,
        join(destination || remotePanel.value?.path || props.session.home, file.name),
        false,
        file.isDir,
      );
    else if (destination || showLocal.value)
      enqueue(
        'download',
        join(
          destination || localPanel.value?.path || '',
          file.name,
          localPanel.value?.separator || '/',
        ),
        file.path,
        false,
        file.isDir,
      );
    else if (file.isDir) {
      const chosen = await open({ directory: true, title: 'Zielordner für den Download wählen' });
      if (typeof chosen === 'string')
        enqueue(
          'download',
          join(chosen, file.name, chosen.includes('\\') ? '\\' : '/'),
          file.path,
          false,
          true,
        );
    } else {
      const local = await save({ defaultPath: file.name, title: 'Datei herunterladen' });
      if (local) enqueue('download', local, file.path, true);
    }
  }
  void processQueue();
}
async function upload(paths?: string[], destination?: string, directory = false) {
  if (!desktop) {
    notify('Dateiauswahl und externe Datei-Drops sind in der Desktop-App verfügbar.');
    return;
  }
  if (!paths) {
    const selected = await open({
      multiple: true,
      directory,
      title: directory ? 'Ordner hochladen' : 'Dateien hochladen',
    });
    if (!selected) return;
    paths = Array.isArray(selected) ? selected : [selected];
  }
  for (const local of paths)
    enqueue(
      'upload',
      local,
      join(destination || remotePanel.value?.path || props.session.home, basename(local)),
    );
  void processQueue();
}
async function toggleSudo(password: string | null = null) {
  if (!desktop) {
    sudo.value = !sudo.value;
    sudoDialog.value = false;
    sudoPassword.value = '';
    notify('sudo-Modus in der Vorschau umgeschaltet.');
    return;
  }
  sudoBusy.value = true;
  await action(async () => {
    try {
      const result = await invoke<{
        type: 'enabled' | 'disabled' | 'passwordRequired';
        message?: string;
      }>('remote_sudo', { id: props.session.id, enabled: !sudo.value, password });
      if (result.type === 'passwordRequired') {
        sudoDialog.value = true;
        return;
      }
      sudo.value = result.type === 'enabled';
      sudoDialog.value = false;
      await remotePanel.value?.refresh();
    } finally {
      sudoPassword.value = '';
    }
  });
  sudoBusy.value = false;
}
async function createFolder() {
  if (
    !mkdir.value ||
    !folderName.value ||
    /[\\/\0]/.test(folderName.value) ||
    ['.', '..'].includes(folderName.value)
  )
    return;
  if (!desktop) {
    notify('Ordner werden in der Desktop-App erstellt.');
    mkdir.value = null;
    return;
  }
  await action(async () => {
    if (mkdir.value!.side === 'local')
      await invoke('local_mkdir', { parent: mkdir.value!.parent, name: folderName.value });
    else
      await invoke('remote_mkdir', {
        id: props.session.id,
        path: join(mkdir.value!.parent, folderName.value),
      });
    mkdir.value = null;
    folderName.value = '';
    await remotePanel.value?.refresh();
    await localPanel.value?.refresh();
  });
}
function requestMkdir(side: 'local' | 'remote', parent: string) {
  folderName.value = '';
  mkdir.value = { side, parent };
}
onMounted(async () => {
  window.addEventListener('pointermove', pointerMove);
  window.addEventListener('pointerup', pointerUp);
  if (desktop) {
    const unlisten = await getCurrentWebview().onDragDropEvent((event) => {
      if (store.activeTab !== props.session.id || !props.visible) return;
      if (event.payload.type === 'leave') {
        nativeDrag.value = false;
        return;
      }
      if (event.payload.type === 'enter' || event.payload.type === 'over') {
        const position = event.payload.position;
        nativeDrag.value =
          dropInfo(
            document.elementFromPoint(
              position.x / window.devicePixelRatio,
              position.y / window.devicePixelRatio,
            ),
          )?.side === 'remote';
      }
      if (event.payload.type === 'drop') {
        nativeDrag.value = false;
        const position = event.payload.position;
        const target = dropInfo(
          document.elementFromPoint(
            position.x / window.devicePixelRatio,
            position.y / window.devicePixelRatio,
          ),
        );
        if (target?.side === 'remote') void upload(event.payload.paths, target.path);
      }
    });
    if (disposed) unlisten();
    else cleanup = unlisten;
  }
});
onBeforeUnmount(() => {
  disposed = true;
  for (const job of jobs.value) if (job.status === 'active') void cancelJob(job);
  cleanup?.();
  window.removeEventListener('pointermove', pointerMove);
  window.removeEventListener('pointerup', pointerUp);
});
</script>
<template>
  <div ref="root" class="file-workspace" :class="{ fullscreen, 'with-local': showLocal }">
    <div class="file-workspace-toolbar">
      <span
        ><Icon name="Files" :size="16" /><strong>Dateien</strong
        ><span class="small-badge">{{
          sudo ? 'root' : isFtp(session.protocol) ? session.protocol.toUpperCase() : 'SFTP'
        }}</span
        ><span v-if="session.protocol === 'ftp'" class="muted">Unverschlüsselt</span></span
      >
      <div>
        <button
          class="button secondary small"
          :class="{ selected: showLocal }"
          aria-label="Lokalen Dateibrowser umschalten"
          @click="showLocal = !showLocal"
        >
          <Icon name="PanelLeft" :size="14" /><span>Lokal</span></button
        ><button
          class="icon-button"
          title="Dateien hochladen"
          :disabled="!session.connected"
          @click="upload()"
        >
          <Icon name="Upload" :size="16" /></button
        ><button
          class="icon-button"
          title="Ordner rekursiv hochladen"
          :disabled="!session.connected"
          @click="upload(undefined, undefined, true)"
        >
          <Icon name="FolderPlus" :size="16" /></button
        ><button
          v-if="!isFtp(session.protocol)"
          class="sudo-button"
          :class="{ enabled: sudo }"
          :disabled="sudoBusy || running || !session.connected"
          @click="toggleSudo()"
        >
          <Icon
            :name="sudoBusy ? 'LoaderCircle' : 'ShieldCheck'"
            :size="14"
            :class="{ spin: sudoBusy }"
          />{{ sudo ? 'root aktiv' : 'sudo' }}</button
        ><button
          v-if="session.protocol === 'ssh'"
          class="icon-button"
          :title="fullscreen ? 'Zurück zum Terminal' : 'Dateiansicht maximieren'"
          @click="emit('fullscreen')"
        >
          <Icon :name="fullscreen ? 'Minimize2' : 'Maximize2'" :size="16" />
        </button>
      </div>
    </div>
    <div class="file-panels">
      <FilePanel
        v-show="showLocal"
        ref="localPanel"
        side="local"
        :session="session"
        :active="visible && showLocal"
        :drop-target="dropSide === 'local'"
        @transfer="transfer"
        @drag="startDrag"
        @mkdir="requestMkdir"
      />
      <div v-if="showLocal" class="file-transfer-divider">
        <button
          class="icon-button"
          title="Lokale Auswahl hochladen"
          :disabled="!localPanel?.selectedEntries.length"
          @click="transfer('local', localPanel?.selectedEntries || [])"
        >
          <Icon name="ArrowRight" :size="16" /></button
        ><button
          class="icon-button"
          title="Remote-Auswahl herunterladen"
          :disabled="!remotePanel?.selectedEntries.length"
          @click="transfer('remote', remotePanel?.selectedEntries || [])"
        >
          <Icon name="ChevronLeft" :size="17" />
        </button>
      </div>
      <FilePanel
        ref="remotePanel"
        side="remote"
        :session="session"
        :active="visible"
        :drop-target="dropSide === 'remote' || nativeDrag"
        @transfer="transfer"
        @drag="startDrag"
        @mkdir="requestMkdir"
      />
    </div>
    <div class="transfer-status">
      <span
        ><Icon
          :name="running ? 'LoaderCircle' : failed ? 'ShieldAlert' : 'CheckCheck'"
          :size="13"
          :class="{ spin: running }"
        />{{
          current
            ? `${current.direction === 'upload' ? 'Upload' : 'Download'} · ${current.name}`
            : failed
              ? `${failed} Übertragung(en) fehlgeschlagen`
              : 'Dateien und Ordner zwischen den Ansichten ziehen'
        }}</span
      ><button @click="queueOpen = !queueOpen">
        {{ remaining ? `${remaining} ausstehend` : `${jobs.length} Transfers`
        }}<Icon :name="queueOpen ? 'ChevronDown' : 'ChevronRight'" :size="12" />
      </button>
    </div>
    <div v-if="queueOpen" class="transfer-queue">
      <div v-if="!jobs.length" class="muted">Noch keine Übertragungen.</div>
      <div v-for="job in jobs" :key="job.id" class="transfer-job">
        <Icon
          :name="
            job.status === 'active'
              ? 'LoaderCircle'
              : job.status === 'done'
                ? 'Check'
                : job.status === 'error'
                  ? 'ShieldAlert'
                  : job.direction === 'upload'
                    ? 'Upload'
                    : 'Download'
          "
          :size="14"
          :class="{ spin: job.status === 'active' }"
        />
        <div>
          <strong>{{ job.name }}</strong
          ><small v-if="job.error && job.status !== 'cancelled'">{{ job.error }}</small>
          <small v-else>{{ jobStatus(job) }}</small>
          <progress
            v-if="job.status === 'active' && job.progress && !job.progress.planning"
            :value="job.progress.bytes"
            :max="Math.max(1, job.progress.totalBytes)"
            :aria-label="`Fortschritt ${job.name}`"
          />
          <small
            v-if="job.status === 'active' && job.progress"
            class="transfer-path"
            :title="job.progress.currentPath"
            >{{ job.progress.currentPath }}</small
          >
          <details v-if="job.report?.skipped">
            <summary>Ausgelassene Einträge</summary>
            <small v-for="path in job.report.skippedPaths" :key="path">{{ path }}</small
            ><small v-if="job.report.skipped > job.report.skippedPaths.length"
              >Weitere Einträge ausgelassen.</small
            >
          </details>
        </div>
        <button
          v-if="job.status === 'error' && job.direction === 'download' && job.directory === false"
          class="button secondary small"
          @click="replace = job"
        >
          Mit Ersetzen wiederholen</button
        ><button
          v-if="job.status === 'active'"
          class="icon-button"
          title="Übertragung abbrechen"
          :disabled="job.cancelRequested"
          @click="cancelJob(job)"
        >
          <Icon name="Square" :size="13" /></button
        ><button
          v-if="job.status === 'queued'"
          class="icon-button"
          title="Wartenden Transfer entfernen"
          @click="jobs = jobs.filter((j) => j.id !== job.id)"
        >
          <Icon name="X" :size="13" />
        </button>
      </div>
    </div>
    <Teleport to="body"
      ><div
        v-if="drag?.active"
        class="file-drag-label"
        :style="{ left: drag.x + 14 + 'px', top: drag.y + 14 + 'px' }"
      >
        <Icon :name="drag.side === 'local' ? 'Upload' : 'Download'" :size="15" />{{
          drag.entries.length
        }}
        Datei(en)/Ordner
      </div></Teleport
    >
    <Modal
      v-if="sudoDialog"
      title="sudo-Passwort benötigt"
      subtitle="Für diesen Zugriff wird ein zusätzliches oder anderes Passwort benötigt."
      @close="
        sudoDialog = false;
        sudoPassword = '';
      "
      ><form class="form-body" @submit.prevent="toggleSudo(sudoPassword)">
        <label
          >Anderes sudo-Passwort<input
            v-model="sudoPassword"
            type="password"
            autocomplete="off"
            autofocus
        /></label>
        <p class="muted">Das Passwort bleibt nur für diese Sitzung im Speicher.</p>
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
          ><button class="button primary" :disabled="sudoBusy">sudo aktivieren</button>
        </div>
      </form></Modal
    >
    <Modal v-if="mkdir" title="Neuer Ordner" :subtitle="mkdir.parent" @close="mkdir = null"
      ><form class="form-body" @submit.prevent="createFolder">
        <label>Ordnername<input v-model="folderName" required autofocus /></label>
        <div class="modal-actions"><button class="button primary">Anlegen</button></div>
      </form></Modal
    >
    <Modal
      v-if="replace"
      title="Lokale Datei ersetzen?"
      :subtitle="replace.local"
      @close="replace = null"
      ><div class="form-body">
        <p>Die vorhandene Datei wird erst nach einem erfolgreichen Download ersetzt.</p>
        <div class="modal-actions">
          <button class="button secondary" @click="replace = null">Abbrechen</button
          ><button class="button primary" @click="retryReplace">Ersetzen und erneut laden</button>
        </div>
      </div></Modal
    >
  </div>
</template>
