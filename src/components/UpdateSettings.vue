<script setup lang="ts">
import { messageText, errorText, t as tr, formatLocale } from '../i18n';

import { computed } from 'vue';
import { desktop } from '../composables/useNoryn';
import { updater, updateBusy, checkUpdates } from '../composables/useUpdater';
import Icon from './Icon.vue';
const status = computed(
  () =>
    ({
      get idle() {
        return tr('Bereit für neue Versionen');
      },
      get checking() {
        return tr('Neue Version wird gesucht …');
      },
      available: tr('Noryn {p0} ist verfügbar', { p0: updater.offer?.version || '' }),
      get downloading() {
        return tr('Update wird heruntergeladen …');
      },
      get ready() {
        return tr('Update ist geprüft und bereit');
      },
      get installing() {
        return tr('Update wird installiert …');
      },
      get current() {
        return tr('Du bist auf dem neuesten Stand');
      },
      get error() {
        return tr('Update-Prüfung nicht möglich');
      },
    })[updater.phase],
);
</script>
<template>
  <section class="settings-section" aria-labelledby="updates-heading">
    <div class="settings-section-head">
      <div>
        <h2 id="updates-heading">{{ tr('Updates') }}</h2>
        <p>{{ tr('Neue Versionen aus den öffentlichen GitHub Releases.') }}</p>
      </div>
      <span class="update-version"
        >{{ tr('Noryn') }} {{ updater.version || tr('· Browser-Vorschau') }}</span
      >
    </div>
    <div class="update-status" :class="{ available: !!updater.offer }" role="status">
      <span class="update-status-icon"
        ><Icon :name="updater.offer ? 'Download' : 'ShieldCheck'" :size="24"
      /></span>
      <div>
        <strong>{{ desktop ? status : tr('Desktop-Updates') }}</strong>
        <p>{{ messageText(updater.hint) }}</p>
      </div>
      <button
        v-if="updater.offer"
        class="button primary"
        :disabled="updater.phase === 'installing'"
        @click="updater.dialog = true"
      >
        {{ tr('Update ansehen') }}<Icon name="ArrowRight" :size="16" />
      </button>
      <button
        v-else
        class="button secondary"
        :disabled="!desktop || updateBusy"
        @click="checkUpdates"
      >
        <Icon name="RefreshCw" :class="{ spin: updater.phase === 'checking' }" :size="16" />{{
          tr('Jetzt prüfen')
        }}
      </button>
    </div>
    <label class="checkbox-row update-preference"
      ><input v-model="updater.automatic" type="checkbox" :disabled="!desktop" />{{
        tr('Automatisch nach Updates suchen')
      }}</label
    >
    <p class="update-detail">
      {{
        tr(
          'Beim Start und bei Rückkehr nach mindestens sechs Stunden. Download und Installation startest du selbst.',
        )
      }}
    </p>
    <p v-if="updater.error" class="backup-error" role="alert">{{ errorText(updater.error) }}</p>
    <p v-if="updater.lastChecked" class="update-detail">
      {{ tr('Zuletzt geprüft:') }}
      {{ new Date(updater.lastChecked).toLocaleString(formatLocale()) }}
    </p>
  </section>
</template>
