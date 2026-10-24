<script setup lang="ts">
import { computed } from 'vue';
import { desktop } from '../composables/useNoryn';
import { updater, updateBusy, checkUpdates } from '../composables/useUpdater';
import Icon from './Icon.vue';
const status = computed(
  () =>
    ({
      idle: 'Bereit für neue Versionen',
      checking: 'Neue Version wird gesucht …',
      available: `Noryn ${updater.offer?.version || ''} ist verfügbar`,
      downloading: 'Update wird heruntergeladen …',
      ready: 'Update ist geprüft und bereit',
      installing: 'Update wird installiert …',
      current: 'Du bist auf dem neuesten Stand',
      error: 'Update-Prüfung nicht möglich',
    })[updater.phase],
);
</script>
<template>
  <section class="settings-section" aria-labelledby="updates-heading">
    <div class="settings-section-head">
      <div>
        <h2 id="updates-heading">Updates</h2>
        <p>Neue Versionen aus den öffentlichen GitHub Releases.</p>
      </div>
      <span class="update-version">Noryn {{ updater.version || '· Browser-Vorschau' }}</span>
    </div>
    <div class="update-status" :class="{ available: !!updater.offer }" role="status">
      <span class="update-status-icon"
        ><Icon :name="updater.offer ? 'Download' : 'ShieldCheck'" :size="24"
      /></span>
      <div>
        <strong>{{ desktop ? status : 'Desktop-Updates' }}</strong>
        <p>{{ updater.hint }}</p>
      </div>
      <button
        v-if="updater.offer"
        class="button primary"
        :disabled="updater.phase === 'installing'"
        @click="updater.dialog = true"
      >
        Update ansehen<Icon name="ArrowRight" :size="16" />
      </button>
      <button
        v-else
        class="button secondary"
        :disabled="!desktop || updateBusy"
        @click="checkUpdates"
      >
        <Icon name="RefreshCw" :class="{ spin: updater.phase === 'checking' }" :size="16" />Jetzt
        prüfen
      </button>
    </div>
    <label class="checkbox-row update-preference"
      ><input v-model="updater.automatic" type="checkbox" :disabled="!desktop" />Automatisch nach
      Updates suchen</label
    >
    <p class="update-detail">
      Beim Start und bei Rückkehr nach mindestens sechs Stunden. Download und Installation startest
      du selbst.
    </p>
    <p v-if="updater.error" class="backup-error" role="alert">{{ updater.error }}</p>
    <p v-if="updater.lastChecked" class="update-detail">
      Zuletzt geprüft: {{ new Date(updater.lastChecked).toLocaleString('de-DE') }}
    </p>
  </section>
</template>
