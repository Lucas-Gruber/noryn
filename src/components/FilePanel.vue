<script setup lang="ts">
import { computed, nextTick, onMounted, ref } from 'vue';
import { invoke } from '@tauri-apps/api/core';
import type { ClientSession, LocalDirectory, RemoteEntry } from '../types';
import { desktop, notify } from '../composables/useHarbor';
import { showMenu } from '../composables/useContextMenu';
import Icon from './Icon.vue';
const props = defineProps<{
  side: 'local' | 'remote';
  session: ClientSession;
  active: boolean;
  dropTarget?: boolean;
}>();
const emit = defineEmits<{
  transfer: [side: 'local' | 'remote', entries: RemoteEntry[]];
  drag: [event: PointerEvent, side: 'local' | 'remote', entries: RemoteEntry[]];
  mkdir: [side: 'local' | 'remote', parent: string];
}>();
const path = ref(props.side === 'remote' ? props.session.home : '');
const separator = ref('/');
const parent = ref<string | null>(null);
const input = ref('');
const editingPath = ref(false);
const pathField = ref<HTMLInputElement>();
const entries = ref<RemoteEntry[]>([]);
const selected = ref(new Set<string>());
const filter = ref('');
const hidden = ref(false);
const busy = ref(false);
const error = ref('');
const locations = ref<{ name: string; path: string }[]>([]);
const history = ref<string[]>([]);
const historyIndex = ref(-1);
let request = 0;
const visibleEntries = computed(() =>
  entries.value.filter(
    (e) =>
      (hidden.value || !e.name.startsWith('.')) &&
      e.name.toLowerCase().includes(filter.value.toLowerCase()),
  ),
);
const selectedEntries = computed(() => entries.value.filter((e) => selected.value.has(e.path)));
const breadcrumbs = computed(() => {
  const normalized = path.value.replaceAll('\\', '/');
  const parts = normalized.split('/').filter(Boolean);
  const windows = props.side === 'local' && separator.value === '\\';
  const root = windows
    ? normalized.startsWith('//')
      ? '\\\\' + [parts.shift(), parts.shift()].filter(Boolean).join('\\') + '\\'
      : (parts.shift() || '') + '\\'
    : '/';
  const result = [{ label: windows ? root : '/', path: root }];
  let current = root;
  for (const part of parts) {
    current = current.replace(/[\\/]$/, '') + separator.value + part;
    result.push({ label: part, path: current });
  }
  return result;
});
function size(value: number) {
  return value < 1024
    ? value + ' B'
    : value < 1024 ** 2
      ? (value / 1024).toFixed(1) + ' KB'
      : value < 1024 ** 3
        ? (value / 1024 ** 2).toFixed(1) + ' MB'
        : (value / 1024 ** 3).toFixed(1) + ' GB';
}
function date(value: number) {
  return value
    ? new Intl.DateTimeFormat('de-DE', {
        day: '2-digit',
        month: '2-digit',
        year: '2-digit',
        hour: '2-digit',
        minute: '2-digit',
      }).format(value * 1000)
    : '—';
}
async function navigate(target?: string, record = true) {
  if (props.side === 'remote') {
    let destination = target || path.value;
    if (destination === '~' || destination.startsWith('~/'))
      destination = props.session.home + destination.slice(1);
    if (!destination.startsWith('/'))
      destination = path.value.replace(/\/$/, '') + '/' + destination;
    const parts: string[] = [];
    for (const part of destination.split('/')) {
      if (part === '..') parts.pop();
      else if (part && part !== '.') parts.push(part);
    }
    target = '/' + parts.join('/');
  }
  const ticket = ++request;
  busy.value = true;
  error.value = '';
  try {
    let result: LocalDirectory;
    if (desktop) {
      result =
        props.side === 'local'
          ? await invoke<LocalDirectory>('local_list', { path: target || null })
          : {
              path: target || path.value,
              parent: (target || path.value).replace(/\/?[^/]+\/?$/, '') || '/',
              separator: '/',
              entries: await invoke<RemoteEntry[]>('remote_list', {
                id: props.session.id,
                path: target || path.value,
              }),
            };
    } else {
      const destination = target || (props.side === 'local' ? '/home/lucas' : props.session.home);
      const names =
        props.side === 'local'
          ? ['Documents', 'Downloads', 'projects', '.ssh', 'deploy.yml', 'notes.md']
          : [
              'backups',
              'config',
              'deploy',
              '.ssh',
              'README.md',
              'docker-compose.yml',
              'access.log',
            ];
      result = {
        path: destination,
        parent: destination.replace(/\/?[^/]+\/?$/, '') || '/',
        separator: '/',
        entries: names.map((name, i) => ({
          name,
          path: `${destination.replace(/\/$/, '')}/${name}`,
          isDir: i < 4,
          isLink: false,
          size: i < 4 ? 4096 : 842 * (i + 1),
          modified: 1792756800,
          permissions: i < 4 ? '755' : '644',
        })),
      };
    }
    if (ticket !== request) return;
    if (path.value !== result.path) filter.value = '';
    path.value = result.path;
    parent.value = result.parent;
    separator.value = result.separator;
    input.value = result.path;
    entries.value = result.entries;
    selected.value = new Set();
    editingPath.value = false;
    if (record && history.value[historyIndex.value] !== result.path) {
      history.value = [...history.value.slice(0, historyIndex.value + 1), result.path];
      historyIndex.value = history.value.length - 1;
    }
  } catch (cause) {
    if (ticket === request) error.value = String(cause);
  } finally {
    if (ticket === request) busy.value = false;
  }
}
async function travel(direction: number) {
  const index = historyIndex.value + direction;
  if (index >= 0 && index < history.value.length) {
    await navigate(history.value[index], false);
    if (!error.value) historyIndex.value = index;
  }
}
function select(entry: RemoteEntry, event?: MouseEvent) {
  if (event?.ctrlKey || event?.metaKey) {
    const next = new Set(selected.value);
    next.has(entry.path) ? next.delete(entry.path) : next.add(entry.path);
    selected.value = next;
  } else if (event?.shiftKey && selected.value.size) {
    const start = visibleEntries.value.findIndex((e) => selected.value.has(e.path));
    const end = visibleEntries.value.indexOf(entry);
    selected.value = new Set(
      visibleEntries.value.slice(Math.min(start, end), Math.max(start, end) + 1).map((e) => e.path),
    );
  } else selected.value = new Set([entry.path]);
}
function pointer(event: PointerEvent, entry: RemoteEntry) {
  if (event.button !== 0 || (event.target as Element).closest('button,input')) return;
  const chosen = selected.value.has(entry.path) ? selectedEntries.value : [entry];
  emit('drag', event, props.side, chosen);
}
function context(event: MouseEvent, entry?: RemoteEntry) {
  if (entry && !selected.value.has(entry.path)) selected.value = new Set([entry.path]);
  showMenu(event, [
    ...(entry?.isDir
      ? [{ label: 'Ordner öffnen', icon: 'FolderOpen', action: () => navigate(entry.path) }]
      : []),
    ...(entry
      ? [
          {
            label: props.side === 'local' ? 'Auswahl hochladen' : 'Auswahl herunterladen',
            icon: props.side === 'local' ? 'Upload' : 'Download',
            disabled: selectedEntries.value.every((e) => e.isDir),
            action: () => emit('transfer', props.side, selectedEntries.value),
          },
          {
            label: 'Pfad kopieren',
            icon: 'Copy',
            action: async () => {
              await navigator.clipboard.writeText(entry.path);
              notify('Pfad kopiert');
            },
          },
        ]
      : []),
    {
      label: 'Neuer Ordner',
      icon: 'FolderPlus',
      separator: !!entry,
      action: () => emit('mkdir', props.side, path.value),
    },
    {
      label: hidden.value ? 'Versteckte Dateien ausblenden' : 'Versteckte Dateien anzeigen',
      icon: 'Eye',
      action: () => (hidden.value = !hidden.value),
    },
    {
      label: 'Aktualisieren',
      icon: 'RefreshCw',
      shortcut: 'F5',
      action: () => navigate(path.value, false),
    },
  ]);
}
async function editPath() {
  editingPath.value = true;
  input.value = path.value;
  await nextTick();
  pathField.value?.focus();
  pathField.value?.select();
}
function keys(event: KeyboardEvent) {
  if ((event.ctrlKey || event.metaKey) && event.key === 'l') {
    event.preventDefault();
    void editPath();
  }
  if (
    (event.ctrlKey || event.metaKey) &&
    event.key === 'a' &&
    !(event.target as Element).closest('input')
  ) {
    event.preventDefault();
    selected.value = new Set(visibleEntries.value.map((e) => e.path));
  }
  if (event.key === 'F5') {
    event.preventDefault();
    void navigate(path.value, false);
  }
  if (event.altKey && event.key === 'ArrowUp') {
    event.preventDefault();
    if (parent.value) void navigate(parent.value);
  }
}
onMounted(async () => {
  void navigate(path.value || undefined);
  if (props.side === 'local')
    locations.value = desktop
      ? await invoke('local_locations')
      : [
          { name: 'Persönlicher Ordner', path: '/home/lucas' },
          { name: 'Downloads', path: '/home/lucas/Downloads' },
          { name: 'Dateisystem', path: '/' },
        ];
});
defineExpose({
  navigate,
  refresh: () => navigate(path.value, false),
  path,
  separator,
  selectedEntries,
});
</script>
<template>
  <section
    class="file-panel"
    :class="{ 'drop-target': dropTarget }"
    :data-file-side="side"
    :data-drop-path="path"
    tabindex="-1"
    @keydown="keys"
    @contextmenu="context($event)"
  >
    <div class="file-panel-title">
      <Icon :name="side === 'local' ? 'Monitor' : 'Server'" :size="17" /><strong>{{
        side === 'local' ? 'Dieser Computer' : session.name
      }}</strong
      ><span class="panel-side-label">{{ side === 'local' ? 'LOKAL' : 'REMOTE' }}</span
      ><select
        v-if="side === 'local'"
        aria-label="Lokalen Ort öffnen"
        @change="navigate(($event.target as HTMLSelectElement).value)"
      >
        <option value="" disabled selected>Orte</option>
        <option v-for="location in locations" :key="location.path" :value="location.path">
          {{ location.name }}
        </option>
      </select>
    </div>
    <div class="file-navigation">
      <button
        class="icon-button"
        title="Zurück"
        :disabled="historyIndex < 1 || busy"
        @click="travel(-1)"
      >
        <Icon name="ChevronLeft" :size="17" /></button
      ><button
        class="icon-button"
        title="Vorwärts"
        :disabled="historyIndex >= history.length - 1 || busy"
        @click="travel(1)"
      >
        <Icon name="ChevronRight" :size="17" /></button
      ><button
        class="icon-button"
        title="Übergeordneter Ordner · Alt ↑"
        :disabled="!parent || path === parent || busy"
        @click="parent && navigate(parent)"
      >
        <Icon name="ArrowUp" :size="16" /></button
      ><button
        class="icon-button"
        title="Persönlicher Ordner"
        @click="navigate(side === 'local' ? undefined : session.home)"
      >
        <Icon name="Home" :size="15" /></button
      ><button
        class="icon-button"
        title="Aktualisieren · F5"
        :disabled="busy"
        @click="navigate(path, false)"
      >
        <Icon name="RefreshCw" :size="14" :class="{ spin: busy }" />
      </button>
      <div class="file-nav-spacer"></div>
      <button class="icon-button" title="Neuer Ordner" @click="emit('mkdir', side, path)">
        <Icon name="FolderPlus" :size="16" /></button
      ><button
        class="icon-button"
        title="Versteckte Dateien"
        :class="{ selected: hidden }"
        @click="hidden = !hidden"
      >
        <Icon name="Eye" :size="15" />
      </button>
    </div>
    <form v-if="editingPath" class="address-input" @submit.prevent="navigate(input)">
      <input
        ref="pathField"
        v-model="input"
        :aria-label="side === 'local' ? 'Lokaler Pfad' : 'Remote-Pfad'"
        @keydown.escape="editingPath = false"
      /><button class="icon-button" title="Pfad öffnen">
        <Icon name="ArrowRight" :size="15" />
      </button>
    </form>
    <div v-else class="file-breadcrumbs" @dblclick="editPath">
      <div>
        <template v-for="(part, i) in breadcrumbs" :key="part.path"
          ><Icon v-if="i" name="ChevronRight" :size="11" /><button
            :title="part.path"
            @click="navigate(part.path)"
          >
            {{ part.label }}
          </button></template
        >
      </div>
      <button class="icon-button" title="Pfad eingeben · Strg L" @click="editPath">
        <Icon name="Pencil" :size="12" />
      </button>
    </div>
    <div class="file-filter">
      <Icon name="Search" :size="14" /><input
        v-model="filter"
        :aria-label="`${side === 'local' ? 'Lokale' : 'Remote'} Dateien filtern`"
        placeholder="In diesem Ordner suchen …"
      /><button
        v-if="selectedEntries.some((e) => !e.isDir)"
        class="button secondary small"
        @click="emit('transfer', side, selectedEntries)"
      >
        <Icon :name="side === 'local' ? 'Upload' : 'Download'" :size="13" />{{
          selectedEntries.length
        }}
      </button>
    </div>
    <div class="file-table-head"><span>Name</span><span>Größe</span><span>Geändert</span></div>
    <div class="file-entries">
      <p v-if="error" class="inline-error">{{ error }}</p>
      <div
        v-for="entry in visibleEntries"
        :key="entry.path"
        class="file-entry"
        :class="{ selected: selected.has(entry.path) }"
        :data-drop-path="entry.isDir ? entry.path : undefined"
        tabindex="0"
        :aria-label="entry.name"
        @click="select(entry, $event)"
        @dblclick="
          entry.isDir ? navigate(entry.path) : side === 'remote' && emit('transfer', side, [entry])
        "
        @pointerdown="pointer($event, entry)"
        @contextmenu="context($event, entry)"
        @keydown.enter.prevent="
          entry.isDir ? navigate(entry.path) : emit('transfer', side, [entry])
        "
      >
        <span class="file-entry-name"
          ><Icon
            :name="entry.isDir ? 'Folder' : 'FileText'"
            :size="17"
            :class="{ 'folder-icon': entry.isDir }" /><span class="ellipsis">{{ entry.name }}</span
          ><Icon v-if="entry.isLink" name="ArrowUpRight" :size="10" /></span
        ><span class="mono file-entry-size">{{ entry.isDir ? '—' : size(entry.size) }}</span
        ><span class="file-entry-date">{{ date(entry.modified) }}</span>
      </div>
      <div v-if="!busy && !visibleEntries.length && !error" class="file-empty">
        <Icon name="FolderOpen" :size="26" />
        <p>{{ filter ? 'Keine passenden Dateien' : 'Dieser Ordner ist leer' }}</p>
      </div>
    </div>
    <div class="file-panel-footer">
      <span>{{
        selected.size ? `${selected.size} ausgewählt` : `${visibleEntries.length} Elemente`
      }}</span
      ><span>{{
        busy
          ? 'Wird geladen …'
          : session.preview
            ? 'Vorschau'
            : side === 'local'
              ? 'Lokales Dateisystem'
              : 'Verschlüsselt über SSH'
      }}</span>
    </div>
  </section>
</template>
