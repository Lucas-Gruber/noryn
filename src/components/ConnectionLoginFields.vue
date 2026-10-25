<script setup lang="ts">
import type { CredentialDraft } from '../types';
import SelectMenu from './SelectMenu.vue';
defineProps<{ form: CredentialDraft; stored?: boolean; ftp?: boolean; disabled?: boolean }>();
</script>
<template>
  <div class="connection-login-fields">
    <div class="form-row">
      <label
        >Benutzername<input
          v-model="form.username"
          autocomplete="off"
          maxlength="512"
          :disabled="disabled"
          placeholder="z. B. lucas"
      /></label>
      <label v-if="!ftp"
        >Anmeldung<SelectMenu
          v-model="form.kind"
          label="Anmeldeverfahren"
          :disabled="disabled"
          :options="[
            { value: 'password', label: 'Passwort / Keyboard-Interactive' },
            { value: 'key', label: 'SSH-Schlüssel' },
            { value: 'agent', label: 'Lokaler SSH-Agent' },
          ]"
      /></label>
    </div>
    <label v-if="form.kind === 'key' && !ftp"
      >Privater SSH-Schlüssel<textarea
        v-model="form.secret"
        rows="4"
        class="mono"
        autocomplete="off"
        :disabled="disabled"
        :placeholder="
          stored
            ? 'Leer lassen, um den gespeicherten Schlüssel zu behalten'
            : '-----BEGIN OPENSSH PRIVATE KEY-----'
        "
      />
    </label>
    <label v-if="form.kind === 'key' && !ftp"
      >Schlüssel-Passphrase<input
        v-model="form.passphrase"
        type="password"
        autocomplete="new-password"
        :disabled="disabled"
        :placeholder="
          stored
            ? 'Leer lassen, um die gespeicherte Passphrase zu behalten'
            : 'Falls der private Schlüssel geschützt ist'
        "
    /></label>
    <label
      >Benutzerpasswort<input
        v-model="form.password"
        type="password"
        autocomplete="new-password"
        :disabled="disabled"
        :placeholder="
          stored
            ? 'Leer lassen, um das gespeicherte Passwort zu behalten'
            : 'Optional bei SSH, z. B. auch für sudo'
        "
        maxlength="65536"
    /></label>
    <p class="muted" v-if="!ftp">
      Schlüssel und Benutzerpasswort können gemeinsam verwendet werden. OTP wird beim Verbinden
      abgefragt.
    </p>
  </div>
</template>
<style scoped>
.connection-login-fields {
  display: flex;
  flex-direction: column;
  gap: 1rem;
}
.connection-login-fields p {
  margin: 0;
  line-height: 1.6;
}
</style>
