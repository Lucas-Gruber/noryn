<script setup lang="ts">
import { reactive, ref } from 'vue';
import { invoke } from '@tauri-apps/api/core';
import { action, desktop, notify, store } from '../composables/useHarbor';
import Modal from './Modal.vue';
import Icon from './Icon.vue';
const adding = ref(false);
const removing = ref('');
const busy = ref(false);
const form = reactive({ id: '', name: '', username: '', kind: 'key', secret: '', passphrase: '' });
function close() {
  adding.value = false;
  form.secret = '';
  form.passphrase = '';
}
async function submit() {
  if (!desktop) {
    close();
    notify('Zugangsdaten werden nur in der Desktop-App gespeichert.');
    return;
  }
  busy.value = true;
  await action(async () => {
    store.view = await invoke('credential_save', { credential: { ...form } });
    close();
    form.name = '';
    form.username = '';
    notify('Zugangsdaten verschlüsselt gespeichert');
  });
  busy.value = false;
}
async function remove() {
  await action(async () => {
    if (desktop) store.view = await invoke('credential_delete', { id: removing.value });
    else {
      store.view.credentials = store.view.credentials.filter((c) => c.id !== removing.value);
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
      <div class="eyebrow">DEIN PERSÖNLICHER TRESOR</div>
      <h1>Zugangsdaten</h1>
      <p>Deine Konten. Verschlüsselt gespeichert. Nur für dich.</p>
    </div>
    <button class="button primary" @click="adding = true">
      <Icon name="Plus" :size="17" />Zugang hinzufügen
    </button>
  </div>
  <div class="security-note">
    <Icon name="ShieldCheck" :size="22" />
    <div>
      <strong>Deine Identität bleibt privat.</strong>
      <p>
        Passwörter und SSH-Schlüssel werden nicht mit dem Team synchronisiert. Ordne sie einer
        Verbindung oder einem ganzen Ordner zu.
      </p>
    </div>
    <span class="small-badge">XChaCha20-Poly1305</span>
  </div>
  <div class="credential-grid">
    <article
      v-for="credential in store.view.credentials"
      :key="credential.id"
      class="credential-card"
    >
      <div class="card-icon"><Icon name="KeyRound" :size="21" /></div>
      <div>
        <h3>{{ credential.name }}</h3>
        <p class="mono">{{ credential.username }}</p>
        <span class="tag">{{
          credential.kind === 'key'
            ? 'SSH-Schlüssel'
            : credential.kind === 'agent'
              ? 'SSH-Agent'
              : 'Passwort'
        }}</span>
      </div>
      <button
        class="icon-button danger-hover"
        aria-label="Zugangsdaten löschen"
        @click="removing = credential.id"
      >
        <Icon name="Trash2" :size="16" />
      </button>
    </article>
  </div>
  <div v-if="!store.view.credentials.length" class="empty-state">
    <Icon name="KeyRound" :size="38" />
    <h3>Dein erstes Konto</h3>
    <p>Füge einen SSH-Schlüssel, ein Passwort oder deinen SSH-Agent hinzu.</p>
    <button class="button secondary" @click="adding = true">Zugangsdaten anlegen</button>
  </div>
  <Modal
    v-if="adding"
    title="Zugangsdaten hinzufügen"
    subtitle="Alle Geheimnisse werden ausschließlich lokal verschlüsselt gespeichert."
    @close="close"
  >
    <form class="form-body" @submit.prevent="submit">
      <label
        >Bezeichnung<input v-model="form.name" placeholder="Mein Firmenkonto" required autofocus
      /></label>
      <div class="form-row">
        <label
          >Benutzername<input
            v-model="form.username"
            placeholder="lucas"
            required
            autocomplete="off" /></label
        ><label
          >Authentifizierung<select v-model="form.kind">
            <option value="key">Privater SSH-Schlüssel</option>
            <option value="password">Passwort</option>
            <option value="agent">Lokaler SSH-Agent</option>
          </select></label
        >
      </div>
      <label v-if="form.kind === 'key'"
        >Privater Schlüssel<textarea
          v-model="form.secret"
          placeholder="-----BEGIN OPENSSH PRIVATE KEY-----"
          rows="7"
          class="mono"
          required
          spellcheck="false"
          autocomplete="off"
        /></label
      ><label v-if="form.kind === 'password'"
        >Passwort<input
          v-model="form.secret"
          type="password"
          required
          autocomplete="new-password" /></label
      ><label v-if="form.kind === 'key'"
        >Schlüssel-Passphrase (optional)<input
          v-model="form.passphrase"
          type="password"
          autocomplete="new-password"
      /></label>
      <p v-if="form.kind === 'agent'" class="muted">
        Verwendet den lokal erreichbaren SSH-Agent. Die verfügbaren Agenten hängen vom
        Betriebssystem und der libssh2-Unterstützung ab.
      </p>
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
