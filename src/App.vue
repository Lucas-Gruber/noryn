<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref } from 'vue';
import { invoke } from '@tauri-apps/api/core';
import type { NodeEntry } from './types';
import {
  action,
  closeSession,
  connect,
  connections,
  credentialFor,
  deleteNode,
  descendants,
  desktop,
  lock,
  notify,
  saveNode,
  setBinding,
  store,
  unlock,
} from './composables/useHarbor';
import Icon from './components/Icon.vue';
import TreeNode from './components/TreeNode.vue';
import Modal from './components/Modal.vue';
import NodeEditor from './components/NodeEditor.vue';
import CredentialsPage from './components/CredentialsPage.vue';
import TeamsPage from './components/TeamsPage.vue';
import SessionPane from './components/SessionPane.vue';

const layout = ref('grid');
const editor = ref<{ node?: NodeEntry; kind: 'folder' | 'connection' } | null>(null);
const deleting = ref<NodeEntry | null>(null);
const help = ref(false);
const password = ref('');
const searchInput = ref<HTMLInputElement>();
const selected = computed(() => store.view.nodes.find((n) => n.id === store.selected));
const effective = computed(() => (selected.value ? credentialFor(selected.value.id) : undefined));
const team = computed(() => store.view.teams.find((t) => t.id === store.workspace));
const workspaceName = computed(() => team.value?.name || 'Privater Workspace');
const rootNodes = computed(() =>
  store.view.nodes
    .filter((n) => n.workspace === store.workspace && !n.parentId)
    .sort(
      (a, b) =>
        (a.kind === 'folder' ? 0 : 1) - (b.kind === 'folder' ? 0 : 1) ||
        a.name.localeCompare(b.name),
    ),
);
const allCount = computed(
  () =>
    store.view.nodes.filter((n) => n.workspace === store.workspace && n.kind === 'connection')
      .length,
);
const favoriteCount = computed(
  () =>
    store.view.nodes.filter(
      (n) => n.workspace === store.workspace && n.kind === 'connection' && n.favorite,
    ).length,
);
const folderName = computed(() => store.view.nodes.find((n) => n.id === store.folder)?.name);
const title = computed(() => (store.favorites ? 'Favoriten' : folderName.value || 'Verbindungen'));
const folderCount = computed(
  () =>
    store.view.nodes.filter((n) => n.workspace === store.workspace && n.kind === 'folder').length,
);
if (!desktop) store.selected = 'web-01';

function navigate(page: typeof store.page) {
  store.page = page;
  store.activeTab = 'connections';
}
function workspaceChange(value: string) {
  store.workspace = value;
  store.folder = 'all';
  store.selected = '';
  store.favorites = false;
  navigate('connections');
}
function all() {
  navigate('connections');
  store.folder = 'all';
  store.favorites = false;
  store.selected = '';
}
function add(kind: 'folder' | 'connection' = 'connection') {
  editor.value = { kind };
}
function edit(node: NodeEntry) {
  editor.value = { node, kind: node.kind };
}
async function move(id: string, parentId: string | null) {
  const node = store.view.nodes.find((n) => n.id === id);
  if (!node || descendants(id).has(parentId || '') || node.parentId === parentId) return;
  await action(async () => {
    await saveNode({ ...node, parentId });
    notify('Eintrag verschoben');
  });
}
async function favorite(node: NodeEntry) {
  await action(() => saveNode({ ...node, favorite: !node.favorite }));
}
async function submitUnlock() {
  await action(async () => {
    await unlock(password.value);
    password.value = '';
  });
}
function acceptHostKey() {
  const challenge = store.challenge;
  if (!challenge) return;
  const node = store.view.nodes.find((n) => n.id === challenge.nodeId);
  if (node) return action(() => connect(node, challenge.fingerprint));
}
function shortcuts(event: KeyboardEvent) {
  if (!(event.ctrlKey || event.metaKey) || !store.unlocked) return;
  if (event.key === 'k') {
    event.preventDefault();
    all();
    setTimeout(() => searchInput.value?.focus(), 0);
  }
  if (event.key === 'n') {
    event.preventDefault();
    add();
  }
}
onMounted(() => {
  window.addEventListener('keydown', shortcuts);
  if (desktop)
    void action(async () => {
      store.exists = await invoke('vault_exists');
    });
});
onBeforeUnmount(() => window.removeEventListener('keydown', shortcuts));
</script>

<template>
  <div v-if="!store.unlocked" class="unlock-screen">
    <div class="unlock-brand">
      <span class="brand-mark"><Icon name="Anchor" :size="26" /></span
      ><span>harbor<span class="brand-period">.</span></span>
    </div>
    <div class="unlock-card">
      <div class="unlock-icon"><Icon name="LockKeyhole" :size="26" /></div>
      <div class="eyebrow">DEIN SICHERER ANKERPLATZ</div>
      <h1>{{ store.exists ? 'Willkommen zurück.' : 'Deine Verbindungen. Dein Harbor.' }}</h1>
      <p>
        {{
          store.exists
            ? 'Entsperre deinen persönlichen Tresor.'
            : 'Erstelle deinen lokalen Tresor für SSH-Zugänge, private Verbindungen und Team-Workspaces.'
        }}
      </p>
      <form @submit.prevent="submitUnlock">
        <label
          >{{ store.exists ? 'Tresor-Passwort' : 'Neues Tresor-Passwort'
          }}<input
            v-model="password"
            type="password"
            :minlength="store.exists ? 1 : 12"
            required
            autofocus
            :autocomplete="store.exists ? 'current-password' : 'new-password'"
            placeholder="Mindestens 12 Zeichen" /></label
        ><button class="button primary" :disabled="store.loading">
          <Icon
            :name="store.loading ? 'LoaderCircle' : 'ArrowRight'"
            :class="{ spin: store.loading }"
            :size="17"
          />{{ store.exists ? 'Tresor entsperren' : 'Tresor erstellen' }}
        </button>
      </form>
      <p class="unlock-note"><Icon name="ShieldCheck" :size="14" />Argon2id · XChaCha20-Poly1305</p>
      <p v-if="!store.exists" class="muted">
        Bewahre dein Passwort sicher auf. Es gibt keine Passwort-Wiederherstellung.
      </p>
    </div>
    <span class="unlock-footer">REMOTE WORKSPACE · BUILT FOR YOUR INFRASTRUCTURE</span>
  </div>

  <div v-else class="app-shell">
    <nav class="icon-rail" aria-label="Hauptnavigation">
      <button class="brand-mark" title="Harbor" @click="all">
        <Icon name="Anchor" :size="25" />
      </button>
      <div class="rail-divider"></div>
      <button
        class="rail-button"
        :class="{ active: store.page === 'connections' }"
        title="Verbindungen"
        @click="navigate('connections')"
      >
        <Icon name="Server" :size="20" /></button
      ><button
        class="rail-button"
        :class="{ active: store.page === 'credentials' }"
        title="Zugangsdaten"
        @click="navigate('credentials')"
      >
        <Icon name="KeyRound" :size="20" /></button
      ><button
        class="rail-button"
        :class="{ active: store.page === 'teams' }"
        title="Team-Sync"
        @click="navigate('teams')"
      >
        <Icon name="Users" :size="20" />
      </button>
      <div class="rail-bottom">
        <button class="rail-button" title="Hilfe & Projektstatus" @click="help = true">
          <Icon name="CircleHelp" :size="20" /></button
        ><button class="rail-button" title="Tresor sperren" @click="action(lock)">
          <Icon name="LockKeyhole" :size="19" />
        </button>
        <div class="avatar">L</div>
      </div>
    </nav>

    <aside class="sidebar">
      <div class="wordmark">harbor<span>.</span><span class="version">BETA</span></div>
      <div class="workspace-switch">
        <div class="workspace-symbol">
          <Icon :name="team ? 'Users' : 'LockKeyhole'" :size="18" />
        </div>
        <div>
          <span class="workspace-label">WORKSPACE</span
          ><select
            :value="store.workspace"
            aria-label="Arbeitsbereich wechseln"
            @change="workspaceChange(($event.target as HTMLSelectElement).value)"
          >
            <option v-for="t in store.view.teams" :key="t.id" :value="t.id">{{ t.name }}</option>
            <option value="private">Privater Workspace</option>
          </select>
        </div>
        <Icon name="ChevronsUpDown" :size="14" />
      </div>
      <div class="sidebar-section">
        <button
          class="sidebar-link"
          :class="{
            active: store.folder === 'all' && !store.favorites && store.page === 'connections',
          }"
          @click="all"
        >
          <Icon name="LayoutGrid" :size="16" />Alle Verbindungen<span class="nav-count">{{
            allCount
          }}</span></button
        ><button
          class="sidebar-link"
          :class="{ active: store.favorites }"
          @click="
            all();
            store.favorites = true;
          "
        >
          <Icon name="Star" :size="16" />Favoriten<span class="nav-count">{{ favoriteCount }}</span>
        </button>
      </div>
      <div class="section-label">
        <span>{{ team ? 'TEAM-VERBINDUNGEN' : 'MEINE VERBINDUNGEN' }}</span
        ><button class="icon-button" title="Neuer Ordner" @click="add('folder')">
          <Icon name="Plus" :size="14" />
        </button>
      </div>
      <div
        class="tree"
        @dragover.prevent
        @drop="
          (e) => {
            const id = e.dataTransfer?.getData('application/harbor-node');
            if (id && e.target === e.currentTarget) move(id, null);
          }
        "
      >
        <TreeNode v-for="node in rootNodes" :key="node.id" :node="node" @edit="edit" @move="move" />
        <div v-if="!rootNodes.length" class="tree-empty">
          Noch keine Einträge.<br /><button @click="add('folder')">
            Ersten Ordner anlegen <Icon name="ArrowRight" :size="12" />
          </button>
        </div>
        <button class="add-folder" @click="add('folder')">
          <Icon name="FolderPlus" :size="15" />Ordner hinzufügen
        </button>
      </div>
      <div class="private-section">
        <div class="section-label">PERSÖNLICH</div>
        <button
          class="sidebar-link"
          :class="{ active: store.workspace === 'private' }"
          @click="workspaceChange('private')"
        >
          <Icon name="LockKeyhole" :size="16" />Private Verbindungen<Icon
            name="ChevronRight"
            :size="13"
            class="push-right"
          /></button
        ><button
          class="sidebar-link"
          :class="{ active: store.page === 'credentials' }"
          @click="navigate('credentials')"
        >
          <Icon name="KeyRound" :size="16" />Meine Zugangsdaten
        </button>
      </div>
      <div class="sidebar-bottom">
        <button class="sync-status" @click="navigate('teams')">
          <span class="sync-status-icon"><Icon name="ShieldCheck" :size="19" /></span
          ><span
            ><strong>{{ team ? 'Verschlüsselter Team-Sync' : 'Lokal verschlüsselt' }}</strong
            ><small>{{
              !desktop
                ? 'Vorschau mit Beispieldaten'
                : team?.dirty
                  ? 'Änderungen zum Hochladen'
                  : team?.revision
                    ? 'Letzter Stand synchronisiert'
                    : team
                      ? 'Sync noch nicht gestartet'
                      : 'Deine Daten bleiben bei dir'
            }}</small></span
          ><Icon name="ChevronRight" :size="13" />
        </button>
        <div class="sidebar-foot">
          <span class="status-dot"></span>Lokaler Tresor entsperrt<Icon
            name="LockKeyhole"
            :size="11"
          />
        </div>
      </div>
    </aside>

    <main class="main-shell">
      <header class="topbar">
        <div class="breadcrumb">
          <Icon name="Home" :size="14" /><Icon name="ChevronRight" :size="12" /><span>{{
            workspaceName
          }}</span
          ><Icon name="ChevronRight" :size="12" /><strong>{{
            store.page === 'teams'
              ? 'Team-Sync'
              : store.page === 'credentials'
                ? 'Zugangsdaten'
                : folderName || 'Verbindungen'
          }}</strong>
        </div>
        <div class="topbar-right">
          <span class="topbar-badge"
            ><Icon :name="desktop ? 'ShieldCheck' : 'Eye'" :size="13" />{{
              desktop ? 'Local first. Always.' : 'Browser-Vorschau'
            }}</span
          ><button class="icon-button" title="Workspace-Einstellungen" @click="navigate('teams')">
            <Icon name="Settings2" :size="17" />
          </button>
        </div>
      </header>
      <div class="tab-bar">
        <button
          class="workspace-tab"
          :class="{ active: store.activeTab === 'connections' }"
          @click="store.activeTab = 'connections'"
        >
          <Icon name="LayoutGrid" :size="14" />Workspace
        </button>
        <div
          v-for="session in store.sessions"
          :key="session.id"
          class="session-tab"
          :class="{ active: store.activeTab === session.id }"
        >
          <button @click="store.activeTab = session.id">
            <span class="status-dot" :class="{ offline: !session.connected }"></span
            >{{ session.name }}</button
          ><button
            class="tab-close"
            :aria-label="`${session.name} schließen`"
            @click="action(() => closeSession(session.id))"
          >
            <Icon name="X" :size="12" />
          </button>
        </div>
        <button
          class="new-tab"
          title="Neue Verbindung"
          @click="
            all();
            add();
          "
        >
          <Icon name="Plus" :size="15" /></button
        ><span v-if="store.sessions.length" class="sessions-count"
          >{{ store.sessions.filter((s) => s.connected).length }} aktive Sitzungen</span
        >
      </div>

      <div v-show="store.activeTab === 'connections'" class="workspace-content">
        <div v-if="store.page === 'connections'" class="connections-layout">
          <section class="connections-main">
            <div class="page-head">
              <div>
                <div class="eyebrow">DEIN REMOTE WORKSPACE</div>
                <h1>{{ title }}<span class="title-dot">.</span></h1>
                <p>Alles verbunden. Sicher organisiert.</p>
              </div>
              <button class="button primary" @click="add()">
                <Icon name="Plus" :size="17" />Neue Verbindung
              </button>
            </div>
            <div class="overview-strip">
              <span
                ><Icon name="Server" :size="15" /><strong>{{ allCount }}</strong> Verbindungen</span
              ><span
                ><Icon name="Folder" :size="15" /><strong>{{ folderCount }}</strong> Ordner</span
              ><span
                ><Icon :name="team ? 'Users' : 'LockKeyhole'" :size="15" />{{
                  team ? 'Mit dem Team geteilt' : 'Nur für dich sichtbar'
                }}</span
              ><span class="overview-lock"
                ><Icon name="ShieldCheck" :size="14" />{{
                  team ? 'E2E-verschlüsselt' : 'Privater Tresor'
                }}</span
              >
            </div>
            <div class="connections-toolbar">
              <div class="search-field">
                <Icon name="Search" :size="16" /><input
                  ref="searchInput"
                  v-model="store.query"
                  placeholder="Verbindungen suchen …"
                  aria-label="Verbindungen suchen"
                /><kbd>Ctrl K</kbd>
              </div>
              <select
                v-model="store.protocol"
                class="protocol-filter"
                aria-label="Protokoll filtern"
              >
                <option value="all">Alle Protokolle</option>
                <option value="ssh">SSH</option>
                <option value="sftp">SFTP</option>
                <option value="rdp">RDP</option>
                <option value="vnc">VNC</option>
                <option value="ftp">FTP</option>
              </select>
              <div class="view-switch">
                <button
                  :class="{ active: layout === 'grid' }"
                  title="Kachelansicht"
                  @click="layout = 'grid'"
                >
                  <Icon name="LayoutGrid" :size="15" /></button
                ><button
                  :class="{ active: layout === 'list' }"
                  title="Listenansicht"
                  @click="layout = 'list'"
                >
                  <Icon name="List" :size="16" />
                </button>
              </div>
            </div>
            <div class="collection-heading">
              <span
                >{{
                  store.query
                    ? 'SUCHERGEBNISSE'
                    : store.favorites
                      ? 'DEINE FAVORITEN'
                      : folderName
                        ? folderName.toUpperCase()
                        : 'ALLE VERBINDUNGEN'
                }}<span>{{ connections.length }}</span></span
              ><span>Nach Name sortiert <Icon name="ArrowDown" :size="11" /></span>
            </div>
            <div class="connection-grid" :class="{ 'list-layout': layout === 'list' }">
              <article
                v-for="node in [...connections].sort((a, b) => a.name.localeCompare(b.name))"
                :key="node.id"
                class="connection-card"
                :class="{ selected: store.selected === node.id }"
                draggable="true"
                @dragstart="(e) => e.dataTransfer?.setData('application/harbor-node', node.id)"
                @click="store.selected = node.id"
                @dblclick="action(() => connect(node))"
              >
                <div class="card-top">
                  <div class="card-icon" :class="node.protocol">
                    <Icon
                      :name="
                        node.tags.includes('database')
                          ? 'Database'
                          : node.protocol === 'sftp'
                            ? 'HardDrive'
                            : 'Server'
                      "
                      :size="22"
                    />
                  </div>
                  <span class="protocol-badge" :class="node.protocol">{{
                    node.protocol.toUpperCase()
                  }}</span
                  ><button
                    class="icon-button favorite-button"
                    :class="{ starred: node.favorite }"
                    :title="node.favorite ? 'Aus Favoriten entfernen' : 'Als Favorit speichern'"
                    @click.stop="favorite(node)"
                  >
                    <Icon name="Star" :size="15" /></button
                  ><button class="icon-button" title="Bearbeiten" @click.stop="edit(node)">
                    <Icon name="MoreHorizontal" :size="18" />
                  </button>
                </div>
                <div class="card-description">
                  <h3>{{ node.name }}</h3>
                  <p class="mono">
                    {{ node.host }}<span>:{{ node.port }}</span>
                  </p>
                </div>
                <div class="card-tags">
                  <span
                    v-for="tag in node.tags.slice(0, 3)"
                    :key="tag"
                    class="tag"
                    :class="{ production: tag === 'production' }"
                    >{{ tag }}</span
                  ><span v-if="!node.tags.length" class="no-tag">Keine Kategorien</span>
                </div>
                <div class="card-footer">
                  <span
                    ><Icon name="KeyRound" :size="12" />{{
                      credentialFor(node.id)?.credential?.username || 'Zugang auswählen'
                    }}<Icon v-if="credentialFor(node.id)?.inherited" name="Link" :size="11" /></span
                  ><button
                    class="connect-link"
                    :disabled="store.loading"
                    @click.stop="action(() => connect(node))"
                  >
                    {{ ['ssh', 'sftp'].includes(node.protocol) ? 'Verbinden' : 'Geplant'
                    }}<Icon name="ArrowUpRight" :size="15" />
                  </button>
                </div>
              </article>
            </div>
            <div v-if="!connections.length" class="empty-state">
              <Icon :name="store.query ? 'Search' : 'Server'" :size="34" />
              <h3>{{ store.query ? 'Keine passenden Verbindungen' : 'Ein neuer Ankerplatz' }}</h3>
              <p>
                {{
                  store.query
                    ? 'Versuche einen anderen Namen, Host oder Tag.'
                    : 'Lege deinen ersten Server an und ordne deine Zugangsdaten zu.'
                }}
              </p>
              <button class="button secondary" @click="store.query ? (store.query = '') : add()">
                {{ store.query ? 'Suche zurücksetzen' : 'Verbindung anlegen' }}
              </button>
            </div>
            <div class="workspace-hint">
              <div class="hint-symbol"><Icon name="KeyRound" :size="18" /></div>
              <div>
                <strong>Ein Team. Individuelle Zugänge.</strong>
                <p>Teilt eure Infrastruktur. Jeder verbindet sich mit dem eigenen Konto.</p>
              </div>
              <button
                class="icon-button"
                title="Zugangsdaten verwalten"
                @click="navigate('credentials')"
              >
                <Icon name="ArrowRight" :size="17" />
              </button>
            </div>
            <div class="workspace-footer">
              <span
                ><Icon name="LockKeyhole" :size="11" />{{
                  desktop
                    ? 'Deine Zugangsdaten bleiben auf diesem Gerät.'
                    : 'Vorschau · Keine echten Verbindungen oder gespeicherten Geheimnisse.'
                }}</span
              ><span>HARBOR v0.1</span>
            </div>
          </section>

          <aside v-if="selected" class="inspector">
            <div class="inspector-heading">
              <span>{{ selected.kind === 'folder' ? 'ORDNERDETAILS' : 'VERBINDUNGSDETAILS' }}</span
              ><button class="icon-button" title="Details schließen" @click="store.selected = ''">
                <Icon name="X" :size="15" />
              </button>
            </div>
            <div class="inspector-body">
              <div class="inspector-server-icon">
                <Icon
                  :name="
                    selected.kind === 'folder'
                      ? 'Folder'
                      : selected.tags.includes('database')
                        ? 'Database'
                        : 'Server'
                  "
                  :size="29"
                />
              </div>
              <h2>{{ selected.name }}</h2>
              <p class="inspector-subtitle">
                <Icon :name="team ? 'Users' : 'LockKeyhole'" :size="12" />{{ workspaceName
                }}<span v-if="selected.kind === 'connection'" class="protocol-badge">{{
                  selected.protocol.toUpperCase()
                }}</span>
              </p>
              <button
                v-if="selected.kind === 'connection'"
                class="button primary inspector-connect"
                :disabled="store.loading"
                @click="action(() => connect(selected!))"
              >
                <Icon
                  :name="store.loading ? 'LoaderCircle' : 'Terminal'"
                  :class="{ spin: store.loading }"
                  :size="16"
                />Verbindung öffnen<Icon name="ArrowUpRight" :size="15" />
              </button>
              <div class="inspector-divider"></div>
              <div v-if="selected.kind === 'connection'" class="detail-properties">
                <div>
                  <span>Hostname</span><strong class="mono">{{ selected.host }}</strong>
                </div>
                <div>
                  <span>Port</span><strong class="mono">{{ selected.port }}</strong>
                </div>
                <div>
                  <span>Protokoll</span
                  ><strong>{{
                    ['ssh', 'sftp'].includes(selected.protocol)
                      ? 'SSH / SFTP'
                      : selected.protocol.toUpperCase() + ' · geplant'
                  }}</strong>
                </div>
                <div>
                  <span>Ordner</span
                  ><strong
                    ><Icon name="Folder" :size="12" />{{
                      store.view.nodes.find((n) => n.id === selected?.parentId)?.name ||
                      'Hauptebene'
                    }}</strong
                  >
                </div>
              </div>
              <div class="inspector-credential">
                <div class="inspector-section-title">
                  <Icon name="KeyRound" :size="14" />Meine Zugangsdaten<span class="small-badge"
                    ><Icon name="LockKeyhole" :size="9" />Privat</span
                  >
                </div>
                <select
                  :value="store.view.bindings[selected.id] || ''"
                  aria-label="Persönliche Zugangsdaten auswählen"
                  @change="
                    action(() =>
                      setBinding(selected!.id, ($event.target as HTMLSelectElement).value || null),
                    )
                  "
                >
                  <option value="">
                    {{
                      effective?.inherited && effective.credential
                        ? effective.credential.name + ' (geerbt)'
                        : 'Vom Ordner erben'
                    }}
                  </option>
                  <option
                    v-for="credential in store.view.credentials"
                    :key="credential.id"
                    :value="credential.id"
                  >
                    {{ credential.name }}
                  </option>
                </select>
                <div v-if="effective?.credential" class="credential-summary">
                  <span class="tiny-avatar">{{
                    effective.credential.username.slice(0, 1).toUpperCase()
                  }}</span>
                  <div>
                    <strong>{{ effective.credential.username }}</strong
                    ><small
                      >{{
                        effective.credential.kind === 'key'
                          ? 'SSH-Schlüssel'
                          : effective.credential.kind === 'agent'
                            ? 'SSH-Agent'
                            : 'Passwort'
                      }}{{ effective.inherited ? ' · von ' + effective.source : '' }}</small
                    >
                  </div>
                  <Icon name="CheckCircle2" :size="15" />
                </div>
                <p>
                  <Icon name="LockKeyhole" :size="11" />Nur deine Auswahl. Wird nicht
                  synchronisiert.
                </p>
                <button
                  v-if="!store.view.credentials.length"
                  class="text-button"
                  @click="navigate('credentials')"
                >
                  Zugangsdaten hinzufügen <Icon name="ArrowRight" :size="12" />
                </button>
              </div>
              <div class="inspector-section">
                <div class="inspector-section-title">KATEGORIEN</div>
                <div class="card-tags">
                  <span v-for="tag in selected.tags" :key="tag" class="tag">{{ tag }}</span
                  ><button class="add-tag" title="Kategorien bearbeiten" @click="edit(selected!)">
                    <Icon name="Plus" :size="12" />
                  </button>
                </div>
              </div>
              <div class="inspector-section">
                <div class="inspector-section-title">
                  NOTIZEN<button
                    class="icon-button"
                    title="Notiz bearbeiten"
                    @click="edit(selected!)"
                  >
                    <Icon name="Pencil" :size="12" />
                  </button>
                </div>
                <p class="notes-text">
                  {{
                    selected.notes ||
                    'Noch keine Notizen. Füge deinem Team hilfreiche Informationen hinzu.'
                  }}
                </p>
              </div>
              <div class="inspector-shared-note">
                <Icon
                  :name="selected.workspace === 'private' ? 'LockKeyhole' : 'Users'"
                  :size="15"
                /><span>{{
                  selected.workspace === 'private'
                    ? 'Dieser Eintrag ist nur für dich sichtbar.'
                    : 'Verbindungsdetails werden mit deinem Team geteilt.'
                }}</span>
              </div>
            </div>
            <div class="inspector-actions">
              <button class="button secondary small" @click="edit(selected!)">
                <Icon name="Pencil" :size="13" />Bearbeiten</button
              ><button
                class="icon-button danger-hover"
                title="Eintrag löschen"
                @click="deleting = selected!"
              >
                <Icon name="Trash2" :size="15" />
              </button>
            </div>
          </aside>
        </div>
        <section v-else class="settings-page">
          <CredentialsPage v-if="store.page === 'credentials'" /><TeamsPage v-else />
        </section>
      </div>
      <SessionPane
        v-for="session in store.sessions"
        v-show="store.activeTab === session.id"
        :key="session.id"
        :session="session"
        :visible="store.activeTab === session.id"
      />
    </main>
  </div>

  <NodeEditor v-if="editor" :node="editor.node" :kind="editor.kind" @close="editor = null" />
  <Modal
    v-if="deleting"
    title="Eintrag löschen?"
    :subtitle="`„${deleting.name}“${deleting.kind === 'folder' ? ' und alle enthaltenen Einträge' : ''} werden entfernt. Bei Team-Einträgen wird diese Änderung beim nächsten Upload geteilt.`"
    @close="deleting = null"
    ><div class="modal-actions">
      <button class="button secondary" @click="deleting = null">Abbrechen</button
      ><button
        class="button danger"
        @click="
          action(async () => {
            await deleteNode(deleting!.id);
            deleting = null;
            notify('Eintrag gelöscht');
          })
        "
      >
        Löschen
      </button>
    </div></Modal
  >
  <Modal
    v-if="store.challenge"
    :title="
      store.challenge.previous ? 'SSH-Hostschlüssel geändert' : 'SSH-Hostschlüssel überprüfen'
    "
    :subtitle="store.challenge.host"
    @close="store.challenge = null"
    ><div class="form-body">
      <div class="host-key-icon">
        <Icon :name="store.challenge.previous ? 'ShieldAlert' : 'ShieldCheck'" :size="26" />
      </div>
      <p>
        {{
          store.challenge.previous
            ? 'Der gespeicherte Schlüssel stimmt nicht mehr überein. Dies kann auf einen neu installierten Server oder einen Angriff hindeuten. Prüfe den Fingerabdruck über einen unabhängigen Kanal.'
            : 'Dies ist deine erste Verbindung mit diesem Host. Prüfe den Fingerabdruck, bevor du deine Zugangsdaten an den Server sendest.'
        }}
      </p>
      <code class="fingerprint">{{ store.challenge.fingerprint }}</code>
      <p v-if="store.challenge.previous" class="muted">Bisher: {{ store.challenge.previous }}</p>
      <div class="modal-actions">
        <button class="button secondary" @click="store.challenge = null">Abbrechen</button
        ><button class="button primary" :disabled="store.loading" @click="acceptHostKey()">
          Schlüssel vertrauen
        </button>
      </div>
    </div></Modal
  >
  <Modal
    v-if="help"
    title="Harbor · Remote Workspace"
    subtitle="Tauri 2 · Vue 3 · Rust · Version 0.1"
    @close="help = false"
    ><div class="form-body">
      <p>
        SSH-Terminal, SFTP-Dateibrowser und ein verschlüsselter Team-Baum mit persönlichen
        Zugangsdaten.
      </p>
      <div class="help-row">
        <Icon name="Terminal" :size="18" /><span
          ><strong>SSH & SFTP</strong>Passwort, privater Schlüssel oder SSH-Agent. Dateiupload per
          Drag & Drop in der Desktop-App.</span
        >
      </div>
      <div class="help-row">
        <Icon name="ShieldCheck" :size="18" /><span
          ><strong>sudo-Dateizugriff</strong>Zusätzlicher SSH-Dateizugriff über sudo und Python 3.
          Das Terminal kann separat mit sudo verwendet werden.</span
        >
      </div>
      <div class="help-row">
        <Icon name="Users" :size="18" /><span
          ><strong>Verschlüsselter Team-Sync</strong>Manueller Push/Pull mit Revisionen. Der
          mitgelieferte Server benötigt für externen Zugriff HTTPS.</span
        >
      </div>
      <div class="help-row">
        <Icon name="Monitor" :size="18" /><span
          ><strong>Weitere Protokolle</strong>RDP, VNC und FTP können als Einträge vorbereitet
          werden; deren Verbindungen sind noch nicht implementiert.</span
        >
      </div>
      <p class="muted">
        Start: npm install → npm run tauri dev. Details stehen in README.md und
        docs/architecture.md.
      </p>
    </div></Modal
  >
  <Modal
    v-if="store.error"
    title="Aktion konnte nicht abgeschlossen werden"
    @close="store.error = ''"
    ><div class="form-body">
      <p class="error-message">{{ store.error }}</p>
      <div class="modal-actions">
        <button class="button secondary" @click="store.error = ''">Schließen</button>
      </div>
    </div></Modal
  >
  <div v-if="store.toast" class="toast" role="status">
    <Icon name="CheckCircle2" :size="18" />{{ store.toast
    }}<button aria-label="Hinweis schließen" @click="store.toast = ''">
      <Icon name="X" :size="14" />
    </button>
  </div>
</template>
