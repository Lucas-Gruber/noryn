<script setup lang="ts">
import { messageText, errorText, t as tr, formatLocale } from '../i18n';

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
  closeUpdatedApp,
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
  `${(value / (1024 * 1024)).toLocaleString(formatLocale(), { maximumFractionDigits: 1 })} MB`;
</script>
<template>
  <aside v-if="showNotice" class="update-notice" role="status">
    <span class="update-status-icon"><Icon name="Download" :size="21" /></span>
    <button class="update-notice-label" @click="updater.dialog = true">
      <strong>{{ tr('Noryn') }} {{ updater.offer?.version }}</strong
      ><span>{{
        updater.phase === 'ready'
          ? tr('Bereit zum Installieren')
          : tr('Eine neue Version ist verfügbar')
      }}</span>
    </button>
    <button class="icon-button" :aria-label="tr('Update-Hinweis schließen')" @click="dismiss">
      <Icon name="X" :size="16" />
    </button>
  </aside>
  <Modal
    v-if="updater.dialog && updater.offer"
    :title="tr('Ein Update für Noryn.')"
    :subtitle="
      updater.flatpak
        ? tr('Aktualisierung über deine Flatpak-Paketquelle.')
        : tr('Neue Funktionen und Verbesserungen, direkt aus dem Release.')
    "
    @close="!['installing', 'installed'].includes(updater.phase) && (updater.dialog = false)"
  >
    <div class="update-release-head">
      <span class="update-status-icon"><Icon name="Download" :size="26" /></span>
      <div>
        <strong>{{ tr('Version') }} {{ updater.offer.version }}</strong
        ><span>{{ tr('Aktuell installiert:') }} {{ updater.version || tr('Entwicklung') }}</span>
      </div>
      <span class="update-trust"
        ><Icon name="ShieldCheck" :size="15" />{{
          updater.flatpak
            ? updater.phase === 'installed'
              ? tr('Durch Flatpak geprüft')
              : tr('Signaturprüfung durch Flatpak')
            : updater.phase === 'ready' || updater.phase === 'installing'
              ? tr('Signatur geprüft')
              : tr('Signierte Updates')
        }}</span
      >
    </div>
    <pre class="update-notes">{{ updater.offer.notes }}</pre>
    <p class="update-detail">{{ messageText(updater.hint) }}</p>
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
        :aria-label="tr('Update-Download')"
        :aria-valuenow="downloadPercent ?? undefined"
        aria-valuemin="0"
        aria-valuemax="100"
      >
        <span
          :class="{
            indeterminate:
              downloadPercent === null &&
              (updater.phase === 'downloading' ||
                (updater.flatpak && updater.phase === 'installing')),
          }"
          :style="{
            width:
              updater.phase === 'downloading' || (updater.flatpak && updater.phase === 'installing')
                ? (downloadPercent ?? 35) + '%'
                : '100%',
          }"
        ></span>
      </div>
      <p v-if="updater.phase === 'downloading'">
        {{ bytes(updater.downloaded)
        }}<template v-if="updater.total"> {{ tr('von') }} {{ bytes(updater.total) }}</template> ·
        {{ downloadPercent === null ? tr('Wird geladen') : downloadPercent + ' %' }}
      </p>
      <p v-else-if="updater.phase === 'ready'">
        <Icon name="CheckCircle2" :size="16" />{{
          tr('Download und Signatur geprüft. Bereit für die Installation.')
        }}
      </p>
      <p v-else-if="updater.flatpak">
        <Icon class="spin" name="LoaderCircle" :size="16" />{{
          tr('Flatpak lädt, prüft und installiert das Update …')
        }}
        <template v-if="downloadPercent !== null"> {{ downloadPercent }} %</template>
      </p>
      <p v-else>
        <Icon class="spin" name="LoaderCircle" :size="16" />{{
          tr('Installation läuft. Noryn startet anschließend neu.')
        }}
      </p>
    </div>
    <p v-if="updater.phase === 'installed'" class="update-detail" role="status">
      {{
        tr(
          'Flatpak wurde aktualisiert. Noryn jetzt schließen und über den App-Starter erneut öffnen, um die neue Version zu verwenden.',
        )
      }}
    </p>
    <p v-if="updater.flatpak && updater.phase === 'available'" class="update-detail">
      {{
        tr(
          'Download und Installation erfolgen gemeinsam über Flatpak. Dafür wird dein Tresor gesperrt. Nicht gespeicherte Formulare werden geschlossen.',
        )
      }}
    </p>
    <p v-if="updater.phase === 'ready'" class="update-detail">
      {{
        tr(
          'Beim Installieren wird der Tresor gesperrt und Noryn neu gestartet. Nicht gespeicherte Formulare werden geschlossen.',
        )
      }}
    </p>
    <p v-if="installBlocked && updater.canInstall" class="update-blocked">
      {{
        tr(
          'Vor der Installation bitte Verbindungen und Anmeldungen schließen und laufende Team-Abgleiche abschließen.',
        )
      }}
    </p>
    <p v-if="updater.error" class="backup-error" role="alert">{{ errorText(updater.error) }}</p>
    <div class="modal-actions">
      <button
        class="button secondary"
        :disabled="['installing', 'installed'].includes(updater.phase)"
        @click="updater.dialog = false"
      >
        {{ tr('Später') }}
      </button>
      <button
        v-if="!updater.canInstall || (updater.flatpak && updater.phase === 'available')"
        class="button secondary"
        @click="openReleases"
      >
        <Icon name="ExternalLink" :size="16" />{{ tr('Release öffnen') }}
      </button>
      <button v-if="updater.phase === 'installed'" class="button primary" @click="closeUpdatedApp">
        {{ tr('Noryn schließen') }}
      </button>
      <button
        v-else-if="updater.flatpak && updater.canInstall"
        class="button primary"
        :disabled="installBlocked || (updateBusy && updater.phase !== 'installing')"
        @click="updater.phase === 'installing' ? cancelUpdate() : installUpdate()"
      >
        {{
          updater.phase === 'installing'
            ? tr('Aktualisierung abbrechen')
            : tr('Über Flatpak aktualisieren')
        }}
      </button>
      <button
        v-else-if="updater.canInstall && updater.phase === 'downloading'"
        class="button secondary"
        @click="cancelUpdate"
      >
        {{ tr('Download abbrechen') }}
      </button>
      <button
        v-else-if="
          updater.canInstall && (updater.phase === 'ready' || updater.phase === 'installing')
        "
        class="button primary"
        :disabled="installBlocked || updater.phase === 'installing'"
        @click="installUpdate"
      >
        <Icon name="RefreshCw" :class="{ spin: updater.phase === 'installing' }" :size="16" />{{
          tr('Installieren & neu starten')
        }}
      </button>
      <button
        v-else-if="updater.canInstall"
        class="button primary"
        :disabled="updateBusy"
        @click="downloadUpdate"
      >
        <Icon name="Download" :size="16" />{{ tr('Update herunterladen') }}
      </button>
    </div>
  </Modal>
</template>
