<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref } from 'vue';
import { invoke } from '@tauri-apps/api/core';
import type { NodeEntry } from './types';
import {
  action,
  closeSession,
  connect,
  credentialFor,
  deleteNode,
  desktop,
  lock,
  notify,
  setBinding,
  store,
  unlock,
} from './composables/useHarbor';
import { showMenu } from './composables/useContextMenu';
import Icon from './components/Icon.vue';
import Modal from './components/Modal.vue';
import NodeEditor from './components/NodeEditor.vue';
import CredentialsPage from './components/CredentialsPage.vue';
import TeamsPage from './components/TeamsPage.vue';
import SessionPane from './components/SessionPane.vue';
import ConnectionTree from './components/ConnectionTree.vue';
import ContextMenu from './components/ContextMenu.vue';
import AuthDialog from './components/AuthDialog.vue';
const editor = ref<{ node?: NodeEntry; kind: 'folder' | 'connection' } | null>(null);
const deleting = ref<NodeEntry | null>(null);
const help = ref(false),
  password = ref('');
const selected = computed(() => store.view.nodes.find((n) => n.id === store.selected));
const effective = computed(() => (selected.value ? credentialFor(selected.value.id) : undefined));
const team = computed(() => store.view.teams.find((t) => t.id === store.workspace));
const workspaceName = computed(() => team.value?.name || 'Privater Workspace');
const activeSession = computed(() => store.sessions.find((s) => s.id === store.activeTab));
const pageName = computed(
  () =>
    activeSession.value?.name ||
    (store.page === 'credentials'
      ? 'Zugangsdaten'
      : store.page === 'teams'
        ? 'Team-Sync'
        : store.favorites
          ? 'Favoriten'
          : 'Verbindungen'),
);
if (!desktop) store.selected = 'web-01';
function navigate(page: typeof store.page, favorites = false) {
  store.page = page;
  store.favorites = favorites;
  store.activeTab = 'connections';
}
function workspaceChange(value: string) {
  store.workspace = value;
  store.selected = '';
  store.query = '';
  store.protocol = 'all';
  store.folder = 'all';
  navigate('connections');
}
function add(kind: 'folder' | 'connection' = 'connection', parent?: string | null) {
  store.folder =
    parent === undefined
      ? selected.value?.kind === 'folder'
        ? selected.value.id
        : selected.value?.parentId || 'all'
      : parent || 'all';
  editor.value = { kind };
}
function edit(node: NodeEntry) {
  editor.value = { node, kind: node.kind };
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
  if (node) {
    store.challenge = null;
    return action(() => connect(node, challenge.fingerprint));
  }
}
function shortcuts(event: KeyboardEvent) {
  if (
    !(event.ctrlKey || event.metaKey) ||
    !store.unlocked ||
    document.querySelector('dialog[open]')
  )
    return;
  if (event.key === 'k') {
    event.preventDefault();
    navigate('connections');
    setTimeout(
      () => document.querySelector<HTMLInputElement>('[aria-label="Verbindungen suchen"]')?.focus(),
      0,
    );
  }
  if (event.key === 'n') {
    event.preventDefault();
    add();
  }
}
function backgroundMenu(event: MouseEvent) {
  if ((event.target as Element).closest('input,textarea,[contenteditable="true"]')) return;
  showMenu(event, [
    { label: 'Neue Verbindung', icon: 'Plus', action: () => add() },
    { label: 'Neuer Ordner', icon: 'FolderPlus', action: () => add('folder') },
    {
      label: 'Meine Zugangsdaten',
      icon: 'KeyRound',
      separator: true,
      action: () => navigate('credentials'),
    },
    { label: 'Team-Sync', icon: 'Users', action: () => navigate('teams') },
  ]);
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
      <span class="brand-symbol">n</span><span>noryn<span class="brand-period">.</span></span>
    </div>
    <div class="unlock-card">
      <div class="unlock-icon"><Icon name="LockKeyhole" :size="25" /></div>
      <h1>{{ store.exists ? 'Willkommen zurück.' : 'Dein Workspace. Sicher verbunden.' }}</h1>
      <p>
        {{
          store.exists
            ? 'Entsperre deinen persönlichen Tresor.'
            : 'Ein geschützter Ort für deine Server, Zugänge und Teams.'
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
            :placeholder="store.exists ? 'Dein Passwort' : 'Mindestens 12 Zeichen'" /></label
        ><button class="button primary" :disabled="store.loading">
          <Icon
            :name="store.loading ? 'LoaderCircle' : 'ArrowRight'"
            :class="{ spin: store.loading }"
            :size="16"
          />{{ store.exists ? 'Entsperren' : 'Tresor erstellen' }}
        </button>
      </form>
      <p v-if="!store.exists" class="unlock-note">
        Bewahre dieses Passwort sicher auf. Es gibt keine Passwort-Wiederherstellung.
      </p>
      <span class="unlock-security"
        ><Icon name="ShieldCheck" :size="13" />Lokal gespeichert und verschlüsselt</span
      >
    </div>
  </div>
  <div v-else class="app-shell" @contextmenu="backgroundMenu">
    <aside class="sidebar">
      <button class="wordmark" @click="navigate('connections')">
        <span class="brand-symbol">n</span><span>noryn<span class="brand-period">.</span></span>
      </button>
      <div class="workspace-switch">
        <Icon :name="team ? 'Users' : 'LockKeyhole'" :size="17" />
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
        <Icon name="ChevronsUpDown" :size="13" />
      </div>
      <div class="section-label">VERBINDEN</div>
      <nav class="sidebar-nav">
        <button
          class="sidebar-link"
          :class="{ active: store.page === 'connections' && !store.favorites }"
          @click="navigate('connections')"
        >
          <Icon name="FolderTree" :size="17" />Verbindungen</button
        ><button
          class="sidebar-link"
          :class="{ active: store.page === 'connections' && store.favorites }"
          @click="navigate('connections', true)"
        >
          <Icon name="Star" :size="17" />Favoriten
        </button>
      </nav>
      <div class="section-label manage-label">VERWALTEN</div>
      <nav class="sidebar-nav">
        <button
          class="sidebar-link"
          :class="{ active: store.page === 'credentials' }"
          @click="navigate('credentials')"
        >
          <Icon name="KeyRound" :size="17" />Meine Zugangsdaten</button
        ><button
          class="sidebar-link"
          :class="{ active: store.page === 'teams' }"
          @click="navigate('teams')"
        >
          <Icon name="Users" :size="17" />Team-Sync<span
            v-if="team?.dirty"
            class="unsynced-dot"
            title="Lokale Änderungen"
          ></span>
        </button>
      </nav>
      <div class="sidebar-bottom">
        <div class="privacy-note">
          <Icon name="ShieldCheck" :size="18" />
          <div>
            <strong>Deine Identität bleibt deine.</strong>
            <p>Jeder im Team nutzt seine eigenen Zugangsdaten.</p>
          </div>
        </div>
        <button class="sidebar-link" @click="help = true">
          <Icon name="CircleHelp" :size="16" />Hilfe & Informationen</button
        ><button class="vault-button" @click="action(lock)">
          <span class="vault-symbol"><Icon name="LockKeyhole" :size="16" /></span
          ><span
            ><strong>Persönlicher Tresor</strong
            ><small>{{
              desktop ? 'Entsperrt · nur auf diesem Gerät' : 'Browser-Vorschau'
            }}</small></span
          ><Icon name="LogOut" :size="15" />
        </button>
      </div>
    </aside>
    <main class="main-shell">
      <header class="topbar">
        <div class="breadcrumb">
          <Icon :name="team ? 'Users' : 'LockKeyhole'" :size="14" /><span>{{ workspaceName }}</span
          ><Icon name="ChevronRight" :size="12" /><strong>{{ pageName }}</strong>
        </div>
        <div class="topbar-right">
          <span v-if="!desktop" class="preview-badge"><Icon name="Eye" :size="12" />Vorschau</span
          ><button class="icon-button" title="Neuer Ordner" @click="add('folder')">
            <Icon name="FolderPlus" :size="17" /></button
          ><button class="button primary small" @click="add()">
            <Icon name="Plus" :size="15" />Neue Verbindung
          </button>
        </div>
      </header>
      <div class="tab-bar">
        <button
          class="workspace-tab"
          :class="{ active: store.activeTab === 'connections' }"
          @click="store.activeTab = 'connections'"
        >
          <Icon name="FolderTree" :size="14" />Workspace
        </button>
        <div
          v-for="session in store.sessions"
          :key="session.id"
          class="session-tab"
          :class="{ active: store.activeTab === session.id }"
        >
          <button @click="store.activeTab = session.id">
            <Icon :name="session.protocol === 'sftp' ? 'Files' : 'Terminal'" :size="13" /><span
              class="status-dot"
              :class="{ offline: !session.connected }"
            ></span
            >{{ session.name }}</button
          ><button
            class="tab-close"
            :aria-label="`${session.name} schließen`"
            @click="action(() => closeSession(session.id))"
          >
            <Icon name="X" :size="12" />
          </button>
        </div>
      </div>
      <div v-show="store.activeTab === 'connections'" class="workspace-content">
        <div v-if="store.page === 'connections'" class="connections-layout">
          <section class="connections-main">
            <div class="page-head">
              <div>
                <h1>
                  {{ store.favorites ? 'Favoriten' : 'Verbindungen'
                  }}<span class="title-dot">.</span>
                </h1>
                <p>
                  {{
                    team
                      ? 'Gemeinsame Infrastruktur, persönliche Zugänge.'
                      : 'Deine privaten Server und Verbindungen.'
                  }}
                </p>
              </div>
              <button class="button secondary small" @click="add('folder')">
                <Icon name="FolderPlus" :size="14" />Neuer Ordner
              </button>
            </div>
            <ConnectionTree @edit="edit" @add="add" @remove="deleting = $event" />
          </section>
          <aside v-if="selected" class="inspector">
            <div class="inspector-heading">
              <span>{{ selected.kind === 'folder' ? 'ORDNER' : 'VERBINDUNG' }}</span
              ><button class="icon-button" title="Details schließen" @click="store.selected = ''">
                <Icon name="X" :size="15" />
              </button>
            </div>
            <div class="inspector-body">
              <div class="inspector-title">
                <span class="detail-icon"
                  ><Icon
                    :name="
                      selected.kind === 'folder'
                        ? 'Folder'
                        : selected.protocol === 'sftp'
                          ? 'Files'
                          : 'Terminal'
                    "
                    :size="23"
                /></span>
                <div>
                  <h2>{{ selected.name }}</h2>
                  <p>
                    {{
                      selected.kind === 'folder'
                        ? 'Zugänge an Untereinträge vererben'
                        : selected.protocol === 'sftp'
                          ? 'Dateien über SSH'
                          : 'Terminal & Dateien'
                    }}
                  </p>
                </div>
              </div>
              <button
                v-if="selected.kind === 'connection'"
                class="button primary inspector-connect"
                :disabled="store.loading"
                @click="action(() => connect(selected!))"
              >
                <Icon
                  :name="
                    store.loading
                      ? 'LoaderCircle'
                      : selected.protocol === 'sftp'
                        ? 'Files'
                        : 'Terminal'
                  "
                  :size="15"
                  :class="{ spin: store.loading }"
                />{{ selected.protocol === 'sftp' ? 'Dateien öffnen' : 'Verbinden'
                }}<Icon name="ArrowUpRight" :size="14" />
              </button>
              <div v-if="selected.kind === 'connection'" class="detail-properties">
                <div>
                  <span>Host</span><strong class="mono">{{ selected.host }}</strong>
                </div>
                <div>
                  <span>Port</span><strong class="mono">{{ selected.port }}</strong>
                </div>
                <div>
                  <span>Protokoll</span><strong>{{ selected.protocol.toUpperCase() }}</strong>
                </div>
              </div>
              <div class="inspector-credential">
                <div class="inspector-section-title">
                  <Icon name="KeyRound" :size="14" /><span>Mein Zugang</span
                  ><span class="small-badge"><Icon name="LockKeyhole" :size="9" />Privat</span>
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
                            : 'Passwort / OTP'
                      }}{{
                        effective.credential.hasPassword && effective.credential.kind !== 'password'
                          ? ' + Passwort'
                          : ''
                      }}</small
                    >
                  </div>
                  <Icon name="CheckCircle2" :size="14" />
                </div>
                <p v-if="effective?.inherited" class="inheritance-note">
                  <Icon name="Link" :size="11" />Geerbt von {{ effective.source }}
                </p>
                <p class="private-hint">
                  <Icon name="LockKeyhole" :size="11" />Diese Auswahl wird nicht synchronisiert.
                </p>
                <button
                  v-if="!store.view.credentials.length"
                  class="text-button"
                  @click="navigate('credentials')"
                >
                  Zugang anlegen<Icon name="ArrowRight" :size="12" />
                </button>
              </div>
              <div class="inspector-section">
                <div class="inspector-section-title">
                  <span>Kategorien</span
                  ><button
                    class="icon-button"
                    title="Kategorien bearbeiten"
                    @click="edit(selected!)"
                  >
                    <Icon name="Plus" :size="13" />
                  </button>
                </div>
                <div class="card-tags">
                  <span v-for="tag in selected.tags" :key="tag" class="tag">{{ tag }}</span
                  ><span v-if="!selected.tags.length" class="muted">Keine Kategorien</span>
                </div>
              </div>
              <div class="inspector-section">
                <div class="inspector-section-title">
                  <span>Notizen</span
                  ><button class="icon-button" title="Notiz bearbeiten" @click="edit(selected!)">
                    <Icon name="Pencil" :size="12" />
                  </button>
                </div>
                <p class="notes-text">
                  {{ selected.notes || 'Informationen zum Eintrag hinzufügen …' }}
                </p>
              </div>
              <div class="inspector-shared-note">
                <Icon
                  :name="selected.workspace === 'private' ? 'LockKeyhole' : 'Users'"
                  :size="14"
                /><span>{{
                  selected.workspace === 'private'
                    ? 'Nur für dich sichtbar'
                    : 'Die Verbindungsdetails werden mit dem Team geteilt.'
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
  <AuthDialog v-if="store.authPrompt" :key="store.authPrompt.id" :challenge="store.authPrompt" />
  <Modal
    v-if="deleting"
    title="Eintrag löschen?"
    :subtitle="`„${deleting.name}“${deleting.kind === 'folder' ? ' und alle Untereinträge' : ''} werden entfernt. Bei Team-Einträgen wird dies beim nächsten Upload geteilt.`"
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
      <Icon :name="store.challenge.previous ? 'ShieldAlert' : 'ShieldCheck'" :size="26" />
      <p>
        {{
          store.challenge.previous
            ? 'Der gespeicherte Schlüssel hat sich geändert. Prüfe den Fingerabdruck über einen unabhängigen Kanal, bevor du dem neuen Schlüssel vertraust.'
            : 'Dies ist deine erste Verbindung zu diesem Host. Prüfe den Fingerabdruck, bevor Zugangsdaten an den Server gesendet werden.'
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
    title="Noryn. · Remote Workspace"
    subtitle="SSH, SFTP und persönliche Team-Zugänge"
    @close="help = false"
    ><div class="form-body">
      <p>
        Ein gemeinsamer Verbindungsbaum, persönliche Zugangsdaten und verschlüsselter Team-Sync.
      </p>
      <div class="help-row">
        <Icon name="Terminal" :size="18" /><span
          ><strong>SSH & SFTP</strong>SSH öffnet Terminal und Dateien. SFTP öffnet ausschließlich
          die Dateiansicht, standardmäßig mit lokalem Bereich.</span
        >
      </div>
      <div class="help-row">
        <Icon name="ShieldCheck" :size="18" /><span
          ><strong>sudo ohne Python</strong>Zuerst wird passwortloses sudo geprüft, danach dein
          gespeichertes Benutzerpasswort. Eine gesonderte Eingabe ist nur bei Bedarf nötig.</span
        >
      </div>
      <div class="help-row">
        <Icon name="KeyRound" :size="18" /><span
          ><strong>Schlüssel, Passwort und OTP</strong>Ein Konto kann SSH-Schlüssel und Passwort
          enthalten. Zusätzliche SSH-Abfragen erscheinen beim Verbinden und werden nicht
          gespeichert.</span
        >
      </div>
      <div class="help-row">
        <Icon name="Files" :size="18" /><span
          ><strong>Dateien übertragen</strong>Dateien zwischen lokalem und entferntem Bereich
          ziehen. Rechtsklick bietet weitere Aktionen. Strg L öffnet die Pfadeingabe, Alt ↑ den
          übergeordneten Ordner.</span
        >
      </div>
      <p class="muted">
        RDP, VNC, FTP und rekursive Ordnertransfers sind noch nicht implementiert. Team-Sync erfolgt
        manuell.
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
  <ContextMenu />
  <div v-if="store.toast" class="toast" role="status">
    <Icon name="CheckCircle2" :size="17" />{{ store.toast
    }}<button aria-label="Hinweis schließen" @click="store.toast = ''">
      <Icon name="X" :size="14" />
    </button>
  </div>
</template>
