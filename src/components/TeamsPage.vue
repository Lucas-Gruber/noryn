<script setup lang="ts">
import { reactive, ref } from 'vue';
import { invoke } from '@tauri-apps/api/core';
import type { TeamInfo } from '../types';
import { action, desktop, notify, store } from '../composables/useNoryn';
import Icon from './Icon.vue';
import Modal from './Modal.vue';
import TeamActivity from './TeamActivity.vue';
import { liveStates, restartLiveTeam, syncNow } from '../composables/useLiveSync';
const fieldLabels: Record<string, string> = {
  authentication: 'SSH-Schlüssel / Passphrase',
  entry: 'Eintrag / Löschung',
  name: 'Name',
  parent: 'Ordner',
  host: 'Host',
  port: 'Port',
  protocol: 'Protokoll',
  initialPath: 'Startordner',
  notes: 'Notizen',
  tags: 'Kategorien',
  favorite: 'Favorit',
  username: 'Benutzername',
  kind: 'Typ',
  secret: 'Schlüssel / Passwort',
  password: 'Passwort',
  passphrase: 'Passphrase',
};
const adding = ref(false);
const busy = ref(false);
const editing = ref(false);
const pullConfirm = ref<TeamInfo | null>(null);
const activityTeam = ref<TeamInfo | null>(null);
const form = reactive({
  id: '',
  name: '',
  memberName: '',
  serverUrl: '',
  token: '',
  key: '',
  revision: null,
  dirty: true,
  liveSync: false,
});
function open(team?: TeamInfo) {
  Object.assign(form, {
    id: team?.id || '',
    name: team?.name || '',
    memberName: team?.memberName || '',
    serverUrl: team?.serverUrl || '',
    token: '',
    key: '',
    revision: null,
    dirty: true,
    liveSync: team?.liveSync || false,
  });
  editing.value = !!team;
  adding.value = true;
}
function close() {
  adding.value = false;
  form.token = '';
  form.key = '';
}
async function generate() {
  if (!desktop) {
    notify('Team-Schlüssel werden in der Desktop-App erzeugt.');
    return;
  }
  form.key = await invoke('team_key_generate');
}
async function submit() {
  if (!desktop) {
    notify('Team-Sync ist in der Desktop-App verfügbar.');
    close();
    return;
  }
  busy.value = true;
  await action(async () => {
    const view = await invoke<typeof store.view>('team_save', { team: { ...form } });
    restartLiveTeam(form.id);
    store.view = view;
    store.workspace = form.id;
    store.folder = 'all';
    close();
    notify('Team-Konfiguration gespeichert');
  });
  busy.value = false;
}
async function syncTeam(team: TeamInfo, direction: 'push' | 'pull') {
  pullConfirm.value = null;
  if (!team.memberName?.trim()) {
    open(team);
    notify('Bitte zuerst deinen Namen für dieses Team angeben.');
    return;
  }
  if (!desktop) {
    notify('Browser-Vorschau: Es werden keine Daten an einen Server gesendet.');
    return;
  }
  busy.value = true;
  await action(async () => {
    const result = await invoke<{ view: typeof store.view; message: string }>('team_sync', {
      teamId: team.id,
      direction,
    });
    store.view = result.view;
    store.selected = '';
    notify(result.message);
  });
  busy.value = false;
}
</script>
<template>
  <div class="page-head">
    <div>
      <div class="eyebrow">GEMEINSAM ORGANISIERT</div>
      <h1>Team-Sync</h1>
      <p>Eine Infrastruktur. Für alle. Mit der eigenen Identität.</p>
    </div>
    <button class="button primary" @click="open()">
      <Icon name="Plus" :size="17" />Team hinzufügen
    </button>
  </div>
  <div class="security-note">
    <Icon name="ShieldCheck" :size="24" />
    <div>
      <strong>Vor dem Upload verschlüsselt.</strong>
      <p>
        Der Server speichert nur verschlüsselte Team-Daten. Zugangsdaten werden ausschließlich nach
        ausdrücklicher Freigabe geteilt. Private Einträge und deine Kontoauswahl bleiben auf deinem
        Gerät.
      </p>
    </div>
    <span class="small-badge">Ende-zu-Ende</span>
  </div>
  <article v-for="team in store.view.teams" :key="team.id" class="team-card">
    <div class="team-card-head">
      <div class="card-icon"><Icon name="Users" :size="22" /></div>
      <div>
        <h3>{{ team.name }}</h3>
        <p>{{ team.serverUrl }}</p>
      </div>
      <span class="status-pill" :class="{ amber: team.dirty }"
        ><span class="status-dot"></span
        >{{
          team.dirty
            ? 'Lokale Änderungen'
            : team.revision
              ? 'Synchronisiert'
              : 'Noch nicht synchronisiert'
        }}</span
      ><button class="icon-button" aria-label="Team bearbeiten" @click="open(team)">
        <Icon name="Settings2" :size="17" />
      </button>
    </div>
    <div class="team-card-meta">
      <span v-if="team.memberName"><Icon name="Users" :size="14" />Du: {{ team.memberName }}</span>
      <button v-else class="button secondary small" @click="open(team)">
        Deinen Namen ergänzen
      </button>
      <span
        ><Icon name="Folder" :size="14" />{{
          store.view.nodes.filter((n) => n.workspace === team.id).length
        }}
        Einträge</span
      ><span class="mono">{{ team.id }}</span
      ><span><Icon name="LockKeyhole" :size="13" />Persönliche Konten bleiben lokal</span>
    </div>
    <div
      v-if="team.liveSync"
      class="team-live-status"
      :class="liveStates[team.id]?.phase"
      role="status"
    >
      <Icon
        :name="liveStates[team.id]?.phase === 'syncing' ? 'RefreshCw' : 'Activity'"
        :size="16"
      />
      <div>
        <strong>Live-Sync</strong>
        <p>{{ liveStates[team.id]?.message || 'Wird verbunden …' }}</p>
        <ul v-if="liveStates[team.id]?.conflicts.length">
          <li
            v-for="conflict in liveStates[team.id]!.conflicts"
            :key="`${conflict.object}:${conflict.id}`"
          >
            {{ conflict.name }} ·
            {{ conflict.fields.map((field) => fieldLabels[field] || field).join(', ') }}
          </li>
        </ul>
        <p v-if="liveStates[team.id]?.phase === 'conflict'">
          Bitte lokale Daten exportieren, bevor du den Serverstand übernimmst. Es werden keine
          Änderungen automatisch verworfen.
        </p>
      </div>
      <button
        class="button secondary small"
        :disabled="busy || liveStates[team.id]?.phase === 'syncing'"
        @click="syncNow(team.id, true)"
      >
        Jetzt abgleichen
      </button>
    </div>
    <div class="team-card-actions">
      <span>{{
        team.liveSync
          ? 'Änderungen werden automatisch abgeglichen.'
          : 'Manueller Abgleich mit Konflikterkennung.'
      }}</span
      ><button class="button secondary small" @click="activityTeam = team">
        <Icon name="Activity" :size="14" />Änderungsverlauf
      </button>
      <button
        class="button secondary small"
        :disabled="busy || liveStates[team.id]?.phase === 'syncing'"
        @click="pullConfirm = team"
      >
        <Icon name="ArrowDown" :size="14" />Vom Server abrufen</button
      ><button
        class="button secondary small"
        :disabled="busy || liveStates[team.id]?.phase === 'syncing'"
        @click="syncTeam(team, 'push')"
      >
        <Icon name="ArrowUp" :size="14" />Änderungen hochladen
      </button>
    </div>
  </article>
  <div v-if="!store.view.teams.length" class="empty-state">
    <Icon name="Users" :size="38" />
    <h3>Verbinde dein Team</h3>
    <p>Verwendet dieselbe Team-ID und denselben Team-Schlüssel auf allen Geräten.</p>
    <button class="button secondary" @click="open()">Team einrichten</button>
  </div>
  <div class="sync-explainer">
    <div>
      <span>01</span>
      <h3>Gemeinsamer Baum</h3>
      <p>Ordner, Hosts, Ports, Kategorien und Notizen werden geteilt.</p>
    </div>
    <div>
      <span>02</span>
      <h3>Persönliche Konten</h3>
      <p>Jeder wählt eigene Zugangsdaten. Auch vererbt pro Ordner.</p>
    </div>
    <div>
      <span>03</span>
      <h3>Eigener Server</h3>
      <p>Den mitgelieferten Sync-Server hinter HTTPS betreiben.</p>
    </div>
  </div>
  <Modal
    v-if="adding"
    :title="editing ? 'Team konfigurieren' : 'Team hinzufügen'"
    subtitle="Team-ID und Schlüssel müssen bei allen Mitgliedern übereinstimmen."
    @close="close"
    ><form class="form-body" @submit.prevent="submit">
      <div class="form-row">
        <label>Teamname<input v-model="form.name" required placeholder="Platform Team" /></label
        ><label
          >Team-ID<input
            v-model="form.id"
            required
            :disabled="editing"
            pattern="[A-Za-z0-9-]{3,80}"
            placeholder="platform-team"
        /></label>
      </div>
      <label
        >Dein Name in diesem Team<input
          v-model="form.memberName"
          required
          maxlength="80"
          placeholder="z. B. Lucas Gruber"
        /><span class="muted"
          >Wird bei deinen Änderungen mit dem Team geteilt. Jedes Mitglied trägt den eigenen Namen
          ein.</span
        ></label
      >
      <label
        >Sync-Server<input
          v-model="form.serverUrl"
          type="url"
          required
          placeholder="https://sync.example.com" /></label
      ><label
        >Server-Token<input
          v-model="form.token"
          type="password"
          :required="!editing"
          :placeholder="
            editing
              ? 'Leer lassen, um den gespeicherten Token zu behalten'
              : 'Mindestens 32 Zeichen'
          "
          autocomplete="new-password" /></label
      ><label
        >Team-Schlüssel (Base64)
        <div class="input-with-button">
          <input
            v-model="form.key"
            :required="!editing"
            :placeholder="
              editing
                ? 'Leer lassen, um den gespeicherten Schlüssel zu behalten'
                : 'Schlüssel von deinem Team einfügen'
            "
            autocomplete="off"
          /><button type="button" class="button secondary small" @click="generate">Erzeugen</button>
        </div></label
      >
      <div class="local-field">
        <Icon name="KeyRound" :size="18" />
        <p>
          Einen neuen Schlüssel vor dem Speichern kopieren und sicher an Teammitglieder weitergeben.
          Er wird danach nicht mehr angezeigt. Bei einem bestehenden Team den vorhandenen Schlüssel
          verwenden.
        </p>
      </div>
      <label class="checkbox-row"
        ><input v-model="form.liveSync" type="checkbox" />Live-Sync für dieses Team
        aktivieren</label
      >
      <p class="muted">
        Speicherungen werden nach kurzer Pause verschlüsselt abgeglichen. Eine offene
        Serververbindung meldet neue Revisionen; keine regelmäßigen Snapshot-Abfragen.
        Widersprüchliche Änderungen bleiben zur Prüfung erhalten. Diese Einstellung gilt nur auf
        diesem Gerät.
      </p>
      <div class="modal-actions">
        <button type="button" class="button secondary" @click="close">Abbrechen</button
        ><button class="button primary" :disabled="busy">Team speichern</button>
      </div>
    </form></Modal
  >
  <Modal
    v-if="pullConfirm"
    title="Team-Baum vom Server übernehmen?"
    subtitle="Der lokale Team-Baum und freigegebene Team-Zugangsdaten werden ersetzt. Nicht hochgeladene Änderungen gehen verloren. Private Einträge, Privatkonten und persönliche Zuordnungen zu weiterhin vorhandenen Konten bleiben erhalten."
    @close="pullConfirm = null"
    ><div class="modal-actions">
      <button class="button secondary" @click="pullConfirm = null">Abbrechen</button
      ><button class="button primary" @click="syncTeam(pullConfirm!, 'pull')">
        Server-Version übernehmen
      </button>
    </div></Modal
  >
  <TeamActivity v-if="activityTeam" :team="activityTeam" @close="activityTeam = null" />
</template>
