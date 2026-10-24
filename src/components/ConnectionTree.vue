<script setup lang="ts">
import SelectMenu from './SelectMenu.vue';
import { computed, ref } from 'vue';
import type { NodeEntry } from '../types';
import { isFileProtocol } from '../protocols';
import { ALL_WORKSPACES, includesWorkspace } from '../workspaces';
import {
  action,
  connect,
  credentialFor,
  descendants,
  notify,
  saveNode,
  store,
} from '../composables/useNoryn';
import { showMenu } from '../composables/useContextMenu';
import Icon from './Icon.vue';
const emit = defineEmits<{
  edit: [node: NodeEntry];
  add: [kind: 'folder' | 'connection', parent?: string | null];
  remove: [node: NodeEntry];
}>();
const collapsed = ref(new Set<string>());
const dropFolder = ref('');
const includedNodes = computed(() => {
  const nodes = store.view.nodes.filter((n) => includesWorkspace(store.workspace, n.workspace));
  const query = store.query.trim().toLowerCase();
  const included = new Set<string>();
  for (const node of nodes) {
    if (
      (!store.favorites || node.favorite) &&
      (store.protocol === 'all' || node.protocol === store.protocol) &&
      (!query || [node.name, node.host, ...node.tags].join(' ').toLowerCase().includes(query))
    ) {
      let current: NodeEntry | undefined = node;
      while (current && !included.has(current.id)) {
        included.add(current.id);
        current = nodes.find((n) => n.id === current?.parentId);
      }
    }
  }
  return nodes.filter((node) => included.has(node.id));
});
const rows = computed(() => {
  const nodes = includedNodes.value;
  const query = store.query.trim();
  const result: { node: NodeEntry; level: number }[] = [];
  function walk(parent: string | null, level: number) {
    for (const node of nodes
      .filter((n) => n.parentId === parent)
      .sort(
        (a, b) =>
          (a.kind === 'folder' ? 0 : 1) - (b.kind === 'folder' ? 0 : 1) ||
          a.name.localeCompare(b.name),
      )) {
      result.push({ node, level });
      if (node.kind === 'folder' && (query || store.favorites || !collapsed.value.has(node.id)))
        walk(node.id, level + 1);
    }
  }
  walk(null, 0);
  return result;
});
const groups = computed(() => {
  const workspaces = [
    ...store.view.teams.map((team) => ({ id: team.id, name: team.name })),
    { id: 'private', name: 'Privater Workspace' },
  ];
  return workspaces
    .filter((workspace) => includesWorkspace(store.workspace, workspace.id))
    .map((workspace) => ({
      ...workspace,
      rows: rows.value.filter(({ node }) => node.workspace === workspace.id),
    }))
    .filter(
      (group) =>
        group.rows.length || (!store.query && !store.favorites && store.protocol === 'all'),
    );
});
function toggle(node: NodeEntry) {
  const next = new Set(collapsed.value);
  next.has(node.id) ? next.delete(node.id) : next.add(node.id);
  collapsed.value = next;
}
function open(node: NodeEntry) {
  if (node.kind === 'folder') toggle(node);
  else void action(() => connect(node));
}
async function move(id: string, parentId: string | null) {
  dropFolder.value = '';
  const node = store.view.nodes.find((n) => n.id === id);
  const parent = store.view.nodes.find((n) => n.id === parentId);
  if (node && parent && node.workspace !== parent.workspace) {
    notify('Einträge können nur innerhalb ihres Workspace verschoben werden.');
    return;
  }
  if (!node || node.parentId === parentId || (parentId && descendants(id).has(parentId))) return;
  await action(async () => {
    await saveNode({ ...node, parentId });
    collapsed.value.delete(parentId || '');
    notify('Eintrag verschoben');
  });
}
function context(event: MouseEvent, node?: NodeEntry) {
  if (node) store.selected = node.id;
  const parent = node?.kind === 'folder' ? node.id : node?.parentId || null;
  showMenu(event, [
    ...(node?.kind === 'connection'
      ? [
          {
            label: isFileProtocol(node.protocol) ? 'Dateien öffnen' : 'Verbinden',
            icon: isFileProtocol(node.protocol) ? 'Files' : 'Terminal',
            shortcut: '↵',
            action: () => connect(node),
          },
        ]
      : []),
    ...(node
      ? [{ label: 'Bearbeiten', icon: 'Pencil', shortcut: 'F2', action: () => emit('edit', node) }]
      : []),
    {
      label: 'Neue Verbindung',
      icon: 'Plus',
      separator: !!node,
      action: () => emit('add', 'connection', parent),
    },
    { label: 'Neuer Ordner', icon: 'FolderPlus', action: () => emit('add', 'folder', parent) },
    ...(node?.kind === 'connection'
      ? [
          {
            label: node.favorite ? 'Aus Favoriten entfernen' : 'Zu Favoriten',
            icon: 'Star',
            separator: true,
            action: () => saveNode({ ...node, favorite: !node.favorite }, node),
          },
          {
            label: 'Verbindung duplizieren',
            icon: 'Copy',
            action: () =>
              saveNode({ ...node, id: crypto.randomUUID(), name: `${node.name} (Kopie)` }),
          },
        ]
      : []),
    ...(node
      ? [
          {
            label: node.kind === 'folder' ? 'Ordner löschen' : 'Verbindung löschen',
            icon: 'Trash2',
            danger: true,
            separator: true,
            action: () => emit('remove', node),
          },
        ]
      : []),
  ]);
}
</script>
<template>
  <div
    class="connection-explorer"
    @contextmenu="context($event)"
    @dragover.prevent
    @drop="
      (e) => {
        if (e.target === e.currentTarget)
          move(e.dataTransfer?.getData('application/noryn-node') || '', null);
      }
    "
  >
    <div class="explorer-toolbar">
      <div class="search-field">
        <Icon name="Search" :size="16" /><input
          v-model="store.query"
          placeholder="Nach Name, Host oder Tag suchen"
          aria-label="Verbindungen suchen"
        /><kbd>Strg K</kbd>
      </div>
      <SelectMenu
        v-model="store.protocol"
        label="Protokoll filtern"
        :options="[
          { value: 'all', label: 'Alle Protokolle' },
          { value: 'ssh', label: 'SSH + SFTP' },
          { value: 'sftp', label: 'SFTP' },
          { value: 'ftps', label: 'FTPS' },
          { value: 'ftp', label: 'FTP' },
          { value: 'rdp', label: 'RDP' },
          { value: 'vnc', label: 'VNC' },
        ]"
      /><button
        class="icon-button"
        title="Alle Ordner einklappen"
        @click="
          collapsed = new Set(store.view.nodes.filter((n) => n.kind === 'folder').map((n) => n.id))
        "
      >
        <Icon name="ChevronsLeft" :size="17" /></button
      ><button class="icon-button" title="Alle Ordner aufklappen" @click="collapsed = new Set()">
        <Icon name="ChevronsRight" :size="17" />
      </button>
    </div>
    <div class="tree-columns">
      <span>Name</span><span>Host</span><span>Protokoll</span><span>Mein Konto</span><span></span>
    </div>
    <div class="tree-rows" role="tree" aria-label="Verbindungen und Ordner">
      <div
        v-for="group in groups"
        :key="group.id"
        class="workspace-tree-group"
        role="group"
        :aria-label="group.name"
      >
        <div v-if="store.workspace === ALL_WORKSPACES" class="workspace-tree-heading">
          <span class="workspace-tree-line"></span>
          <Icon :name="group.id === 'private' ? 'LockKeyhole' : 'Users'" :size="13" />
          <span>{{ group.name }}</span>
          <span class="workspace-tree-line"></span>
        </div>
        <div
          v-for="{ node, level } in group.rows"
          :key="node.id"
          class="explorer-row"
          :class="{
            selected: store.selected === node.id,
            folder: node.kind === 'folder',
            'drop-target': dropFolder === node.id,
          }"
          tabindex="0"
          role="treeitem"
          :aria-level="level + 1"
          :aria-expanded="node.kind === 'folder' ? !collapsed.has(node.id) : undefined"
          :aria-selected="store.selected === node.id"
          draggable="true"
          @dragstart="(e) => e.dataTransfer?.setData('application/noryn-node', node.id)"
          @dragover.stop.prevent="dropFolder = node.kind === 'folder' ? node.id : ''"
          @dragleave="dropFolder = ''"
          @drop.stop.prevent="
            node.kind === 'folder' &&
            move($event.dataTransfer?.getData('application/noryn-node') || '', node.id)
          "
          @click="store.selected = node.id"
          @dblclick="open(node)"
          @keydown.enter.prevent="open(node)"
          @keydown.f2.prevent="emit('edit', node)"
          @keydown.delete.prevent="emit('remove', node)"
          @contextmenu="context($event, node)"
        >
          <div class="tree-name" :style="{ paddingLeft: `${14 + level * 24}px` }">
            <span
              v-if="level"
              class="indent-guide"
              :style="{ left: `${14 + (level - 1) * 24}px` }"
            ></span
            ><button
              v-if="node.kind === 'folder'"
              class="expand-toggle"
              :aria-label="collapsed.has(node.id) ? 'Ordner aufklappen' : 'Ordner einklappen'"
              @click.stop="toggle(node)"
            >
              <Icon
                :name="collapsed.has(node.id) ? 'ChevronRight' : 'ChevronDown'"
                :size="13"
              /></button
            ><span v-else class="expand-spacer"></span
            ><Icon
              :name="
                node.kind === 'folder'
                  ? collapsed.has(node.id)
                    ? 'Folder'
                    : 'FolderOpen'
                  : isFileProtocol(node.protocol)
                    ? 'Files'
                    : 'Terminal'
              "
              :size="17"
            /><span class="ellipsis">{{ node.name }}</span
            ><Icon v-if="node.favorite" name="Star" :size="11" class="subtle-star" />
          </div>
          <span class="host-cell mono ellipsis">{{
            node.kind === 'connection' ? `${node.host}:${node.port}` : ''
          }}</span
          ><span class="protocol-cell"
            ><span v-if="node.kind === 'connection'" class="protocol-badge">{{
              node.protocol.toUpperCase()
            }}</span></span
          ><span class="account-cell ellipsis"
            ><template v-if="credentialFor(node.id)?.credential"
              ><Icon name="KeyRound" :size="12" />{{ credentialFor(node.id)?.credential?.username
              }}<Icon v-if="credentialFor(node.id)?.inherited" name="Link" :size="10" /></template
            ><template v-else-if="node.kind === 'connection'"
              ><span class="muted">Nicht zugeordnet</span></template
            ></span
          ><button
            class="icon-button row-menu"
            :aria-label="`${node.name}: Aktionen`"
            @click.stop="context($event, node)"
          >
            <Icon name="MoreHorizontal" :size="17" />
          </button>
        </div>
        <p
          v-if="!group.rows.length && store.workspace === ALL_WORKSPACES"
          class="workspace-tree-empty"
        >
          Noch keine Einträge in diesem Workspace.
        </p>
      </div>
      <div v-if="!rows.length" class="empty-state">
        <Icon :name="store.query ? 'Search' : 'FolderTree'" :size="36" />
        <h3>{{ store.query ? 'Keine passenden Einträge' : 'Deine Infrastruktur beginnt hier' }}</h3>
        <p>Ordner und Verbindungen stehen gemeinsam in diesem Baum.</p>
        <button class="button secondary" @click="emit('add', 'connection', null)">
          Verbindung anlegen
        </button>
      </div>
    </div>
    <div class="explorer-footer">
      <span
        >{{ includedNodes.filter((node) => node.kind === 'connection').length }} Verbindungen ·
        {{ includedNodes.filter((node) => node.kind === 'folder').length }} Ordner</span
      ><span>Rechtsklick für Aktionen</span>
    </div>
  </div>
</template>
