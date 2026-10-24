<script setup lang="ts">
import { computed } from 'vue';
import {
  updater,
  updateBusy,
  downloadPercent,
  installBlocked,
  downloadUpdate,
  cancelUpdate,
  installUpdate,
  openReleases,
} from '../composables/useUpdater';
import Icon from './Icon.vue';
import Modal from './Modal.vue';
const showNotice = computed(
  () =>
    updater.offer &&
    !updater.dialog &&
    updater.dismissed !== updater.offer.version &&
    updater.phase !== 'installing',
);
function dismiss() {
  updater.dismissed = updater.offer?.version || '';
}
const bytes = (value: number) =>
  `${(value / (1024 * 1024)).toLocaleString('de-DE', { maximumFractionDigits: 1 })} MB`;
</script>
<template>
  <aside v-if="showNotice" class="update-notice" role="status">
    <span class="update-status-icon"><Icon name="Download" :size="21" /></span>
    <button class="update-notice-label" @click="updater.dialog = true">
      <strong>Noryn {{ updater.offer?.version }}</strong
      ><span>{{
        updater.phase === 'ready' ? 'Bereit zum Installieren' : 'Eine neue Version ist verfügbar'
      }}</span>
    </button>
    <button class="icon-button" aria-label="Update-Hinweis schließen" @click="dismiss">
      <Icon name="X" :size="16" />
    </button>
  </aside>
  <Modal
    v-if="updater.dialog && updater.offer"
    title="Ein Update für Noryn."
    subtitle="Neue Funktionen und Verbesserungen, direkt aus dem Release."
    @close="updater.phase !== 'installing' && (updater.dialog = false)"
  >
    <div class="update-release-head">
      <span class="update-status-icon"><Icon name="Download" :size="26" /></span>
      <div>
        <strong>Version {{ updater.offer.version }}</strong
        ><span>Aktuell installiert: {{ updater.version || 'Entwicklung' }}</span>
      </div>
      <span class="update-trust"
        ><Icon name="ShieldCheck" :size="15" />{{
          updater.phase === 'ready' || updater.phase === 'installing'
            ? 'Signatur geprüft'
            : 'Signierte Updates'
        }}</span
      >
    </div>
    <pre class="update-notes">{{ updater.offer.notes }}</pre>
    <p class="update-detail">{{ updater.hint }}</p>
    <div
      v-if="
        updater.phase === 'downloading' ||
        updater.phase === 'ready' ||
        updater.phase === 'installing'
      "
      class="update-progress-area"
      role="status"
    >
      <div
        class="update-progress"
        role="progressbar"
        aria-label="Update-Download"
        :aria-valuenow="downloadPercent ?? undefined"
        aria-valuemin="0"
        aria-valuemax="100"
      >
        <span
          :class="{ indeterminate: downloadPercent === null && updater.phase === 'downloading' }"
          :style="{
            width: updater.phase === 'downloading' ? (downloadPercent ?? 35) + '%' : '100%',
          }"
        ></span>
      </div>
      <p v-if="updater.phase === 'downloading'">
        {{ bytes(updater.downloaded)
        }}<template v-if="updater.total"> von {{ bytes(updater.total) }}</template> ·
        {{ downloadPercent === null ? 'Wird geladen' : downloadPercent + ' %' }}
      </p>
      <p v-else-if="updater.phase === 'ready'">
        <Icon name="CheckCircle2" :size="16" />Download und Signatur geprüft. Bereit für die
        Installation.
      </p>
      <p v-else>
        <Icon class="spin" name="LoaderCircle" :size="16" />Installation läuft. Noryn startet
        anschließend neu.
      </p>
    </div>
    <p v-if="updater.phase === 'ready'" class="update-detail">
      Beim Installieren wird der Tresor gesperrt und Noryn neu gestartet. Nicht gespeicherte
      Formulare werden geschlossen.
    </p>
    <p v-if="installBlocked && updater.canInstall" class="update-blocked">
      Vor der Installation bitte Verbindungen und Anmeldungen schließen und laufende Team-Abgleiche
      abschließen.
    </p>
    <p v-if="updater.error" class="backup-error" role="alert">{{ updater.error }}</p>
    <div class="modal-actions">
      <button
        class="button secondary"
        :disabled="updater.phase === 'installing'"
        @click="updater.dialog = false"
      >
        Später
      </button>
      <button v-if="!updater.canInstall" class="button primary" @click="openReleases">
        <Icon name="ExternalLink" :size="16" />Release öffnen
      </button>
      <button
        v-else-if="updater.phase === 'downloading'"
        class="button secondary"
        @click="cancelUpdate"
      >
        Download abbrechen
      </button>
      <button
        v-else-if="updater.phase === 'ready' || updater.phase === 'installing'"
        class="button primary"
        :disabled="installBlocked || updater.phase === 'installing'"
        @click="installUpdate"
      >
        <Icon
          name="RefreshCw"
          :class="{ spin: updater.phase === 'installing' }"
          :size="16"
        />Installieren & neu starten
      </button>
      <button v-else class="button primary" :disabled="updateBusy" @click="downloadUpdate">
        <Icon name="Download" :size="16" />Update herunterladen
      </button>
    </div>
  </Modal>
</template>
