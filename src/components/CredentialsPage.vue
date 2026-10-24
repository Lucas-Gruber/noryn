<script setup lang="ts">
import SelectMenu from './SelectMenu.vue';
import { computed, reactive, ref, watch } from 'vue';
import { invoke } from '@tauri-apps/api/core';
import type { CredentialInfo } from '../types';
import { action, desktop, notify, store } from '../composables/useNoryn';
import Modal from './Modal.vue';
import Icon from './Icon.vue';
import CredentialOrganizer from './CredentialOrganizer.vue';
import { credentialFolderOptions } from '../composables/useCredentialFolders';
const adding = ref(false);
const removing = ref('');
const busy = ref(false);
const editRevision = ref(0);
const editError = ref('');
const vaultFilter = ref('all');
const shared = ref(false);
const teamId = ref('');
const currentFolder = ref<string | null>(null);
const credentialFolder = ref<string | null>(null);
const vaultCredentials = computed(() =>
  store.view.credentials.filter(
    (credential) =>
      vaultFilter.value === 'all' ||
      (vaultFilter.value === 'private'
        ? !credential.teamId
        : credential.teamId === vaultFilter.value),
  ),
);
const form = reactive({
  id: '',
  name: '',
  username: '',
  kind: 'key',
  secret: '',
  passphrase: '',
  password: '',
});
watch(
  () => form.kind,
  (kind) => {
    if (kind === 'agent') shared.value = false;
  },
);
function edit(credential?: CredentialInfo) {
  editRevision.value = store.view.localRevision;
  editError.value = '';
  credentialFolder.value = credential
    ? store.view.credentialLocations[credential.id] || null
    : currentFolder.value;
  shared.value =
    !!credential?.teamId || (!credential && !['all', 'private'].includes(vaultFilter.value));
  teamId.value =
    credential?.teamId ||
    (store.view.teams.some((team) => team.id === vaultFilter.value)
      ? vaultFilter.value
      : store.view.teams[0]?.id || '');
  Object.assign(form, {
    id: credential?.id || '',
    name: credential?.name || '',
    username: credential?.username || '',
    kind: credential?.kind || 'key',
    secret: '',
    passphrase: '',
    password: '',
  });
  adding.value = true;
}
function close() {
  adding.value = false;
  form.secret = '';
  form.passphrase = '';
  form.password = '';
}
async function submit() {
  if (!desktop) {
    close();
    notify('Zugangsdaten werden nur in der Desktop-App gespeichert.');
    return;
  }
  busy.value = true;
  editError.value = '';
  try {
    store.view = await invoke('credential_save', {
      credential: { ...form, teamId: shared.value ? teamId.value : null },
      folderId: credentialFolder.value,
      expectedRevision: editRevision.value,
    });
    close();
    form.name = '';
    form.username = '';
    notify('Zugangsdaten verschlüsselt gespeichert');
  } catch (e) {
    editError.value = String(e);
  }
  busy.value = false;
}
async function remove() {
  await action(async () => {
    if (desktop) store.view = await invoke('credential_delete', { id: removing.value });
    else {
      store.view.credentials = store.view.credentials.filter((c) => c.id !== removing.value);
      delete store.view.credentialLocations[removing.value];
      for (const id in store.view.bindings)
        if (store.view.bindings[id] === removing.value) delete store.view.bindings[id];
    }
    removing.value = '';
    notify('Zugangsdaten entfernt');
  });
}
</script>
<template>
  <div class="page-head">
    <div>
      <div class="eyebrow">PRIVATE UND TEAM-TRESORE</div>
      <h1>Zugangsdaten</h1>
      <p>Standardmäßig privat. Auf Wunsch mit deinem Team geteilt.</p>
    </div>
    <button class="button primary" @click="edit()">
      <Icon name="Plus" :size="17" />Zugang hinzufügen
    </button>
  </div>
  <div class="security-note">
    <Icon name="ShieldCheck" :size="22" />
    <div>
      <strong>Du entscheidest, was du teilst.</strong>
      <p>
        Nur ausdrücklich in einem Team-Tresor gespeicherte Konten werden verschlüsselt
        synchronisiert. Die Kontoauswahl an Verbindungen und Ordnern bleibt immer persönlich.
      </p>
    </div>
    <span class="small-badge">Private Auswahl</span>
  </div>
  <div class="credential-vault-tabs">
    <button
      class="button secondary small"
      :class="{ selected: vaultFilter === 'all' }"
      @click="vaultFilter = 'all'"
    >
      Alle Konten
    </button>
    <button
      class="button secondary small"
      :class="{ selected: vaultFilter === 'private' }"
      @click="vaultFilter = 'private'"
    >
      <Icon name="LockKeyhole" :size="13" />Privater Tresor
    </button>
    <button
      v-for="team in store.view.teams"
      :key="team.id"
      class="button secondary small"
      :class="{ selected: vaultFilter === team.id }"
      @click="vaultFilter = team.id"
    >
      <Icon name="Users" :size="13" />{{ team.name }}
    </button>
  </div>
  <CredentialOrganizer
    :credentials="vaultCredentials"
    @edit="edit"
    @remove="removing = $event"
    @navigate="currentFolder = $event"
  />
  <Modal
    v-if="adding"
    :title="form.id ? 'Zugangsdaten bearbeiten' : 'Zugangsdaten hinzufügen'"
    subtitle="Private Konten bleiben lokal. Team-Konten werden Ende-zu-Ende verschlüsselt synchronisiert."
    @close="close"
  >
    <form class="form-body" @submit.prevent="submit">
      <label
        >Bezeichnung<input v-model="form.name" placeholder="Mein Firmenkonto" required autofocus
      /></label>
      <label>
        Persönlicher Ordner<SelectMenu
          v-model="credentialFolder"
          label="Persönlicher Zugangsdaten-Ordner"
          :options="credentialFolderOptions"
        />
      </label>
      <div class="form-row">
        <label
          >Benutzername<input
            v-model="form.username"
            placeholder="lucas"
            required
            autocomplete="off" /></label
        ><label
          >Authentifizierung<SelectMenu
            v-model="form.kind"
            label="Authentifizierung"
            :options="
              [
                { value: 'key', label: 'Privater SSH-Schlüssel' },
                { value: 'password', label: 'Passwort' },
                { value: 'agent', label: 'Lokaler SSH-Agent' },
              ] as const
            "
        /></label>
      </div>
      <label v-if="form.kind === 'key'"
        >Privater Schlüssel<textarea
          v-model="form.secret"
          placeholder="-----BEGIN OPENSSH PRIVATE KEY-----"
          rows="7"
          class="mono"
          :required="!form.id"
          spellcheck="false"
          autocomplete="off"
        /></label
      ><label v-if="form.kind === 'key'"
        >Schlüssel-Passphrase (optional)<input
          v-model="form.passphrase"
          type="password"
          autocomplete="new-password"
      /></label>
      <label
        >{{
          form.kind === 'password'
            ? 'Benutzerpasswort'
            : 'Benutzerpasswort für sudo / weitere Anmeldung (optional)'
        }}<input
          v-model="form.password"
          type="password"
          autocomplete="new-password"
          :placeholder="
            form.id
              ? 'Leer lassen, um das gespeicherte Passwort zu behalten'
              : 'Optional bei OTP oder passwortlosem sudo'
          "
      /></label>
      <p class="muted">
        SSH-Schlüssel und Benutzerpasswort können gemeinsam gespeichert werden. Die
        Schlüssel-Passphrase entschlüsselt nur den privaten Schlüssel. Für OTP erscheint beim
        Verbinden ein separater Dialog.
      </p>
      <p v-if="form.kind === 'agent'" class="muted">
        Verwendet den lokal erreichbaren SSH-Agent. Die verfügbaren Agenten hängen vom
        Betriebssystem und der libssh2-Unterstützung ab.
      </p>
      <label class="checkbox-row"
        ><input
          v-model="shared"
          type="checkbox"
          :disabled="form.kind === 'agent' || !store.view.teams.length"
        />Mit einem Team teilen</label
      >
      <label v-if="shared"
        >Team-Tresor<SelectMenu
          v-model="teamId"
          label="Team-Tresor"
          :options="store.view.teams.map((t) => ({ value: t.id, label: t.name }))"
      /></label>
      <p v-if="shared" class="sharing-note">
        Mitglieder mit dem Team-Schlüssel können diese Zugangsdaten nach dem nächsten Team-Abgleich
        verwenden und ändern. Deine Auswahl am Verbindungseintrag wird nicht geteilt.
      </p>
      <p v-else-if="form.kind === 'agent'" class="muted">
        Ein SSH-Agent ist an dieses Gerät gebunden und bleibt im privaten Tresor.
      </p>
      <p v-if="editError" class="backup-error" role="alert">{{ editError }}</p>
      <div class="modal-actions">
        <button type="button" class="button secondary" @click="close">Abbrechen</button
        ><button class="button primary" :disabled="busy">Verschlüsselt speichern</button>
      </div>
    </form>
  </Modal>
  <Modal
    v-if="removing"
    title="Zugangsdaten entfernen?"
    subtitle="Alle persönlichen Zuordnungen zu diesem Zugang werden ebenfalls entfernt."
    @close="removing = ''"
    ><div class="modal-actions">
      <button class="button secondary" @click="removing = ''">Abbrechen</button
      ><button class="button danger" @click="remove">Entfernen</button>
    </div></Modal
  >
</template>
