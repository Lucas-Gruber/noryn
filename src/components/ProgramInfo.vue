<script setup lang="ts">
import { ref } from 'vue';
import { desktop, notify, store } from '../composables/useNoryn';
import {
  checkUpdates,
  openProject,
  openReleases,
  updateBusy,
  updater,
} from '../composables/useUpdater';
import logo from '../../src-tauri/icons/icon.svg';
import Icon from './Icon.vue';
import Modal from './Modal.vue';
defineEmits<{ close: [] }>();
function switchTab(event: KeyboardEvent) {
  if (!['ArrowLeft', 'ArrowRight', 'Home', 'End'].includes(event.key)) return;
  event.preventDefault();
  tab.value =
    event.key === 'Home'
      ? 'about'
      : event.key === 'End'
        ? 'help'
        : tab.value === 'about'
          ? 'help'
          : 'about';
  document.getElementById(tab.value === 'about' ? 'about-tab' : 'help-tab')?.focus();
}
const tab = ref<'about' | 'help'>('about');
async function copyLocation() {
  try {
    await navigator.clipboard.writeText(store.vaultPath);
    notify('Tresorpfad kopiert');
  } catch {
    notify('Der Pfad konnte nicht kopiert werden.');
  }
}
function releaseDownloads() {
  if (desktop) void openReleases();
  else
    window.open('https://github.com/Lucas-Gruber/noryn/releases', '_blank', 'noopener,noreferrer');
}
async function updates() {
  await checkUpdates();
  updater.dialog = true;
}
</script>
<template>
  <Modal title="Über Noryn" @close="$emit('close')">
    <div class="program-info">
      <div class="about-brand">
        <img :src="logo" alt="" width="88" height="88" />
        <div>
          <h3>Noryn.</h3>
          <span class="about-version">Version {{ updater.version }}</span>
        </div>
      </div>
      <p class="about-description">Deine Verbindungen. Dein Workspace.</p>
      <p class="about-intro">
        Terminal und Dateitransfers an einem Ort – mit persönlichen Zugangsdaten und verschlüsseltem
        Team-Sync.
      </p>
      <div class="about-protocols" aria-label="Unterstützte Protokolle">
        <span>SSH</span><span>SFTP</span><span>FTP</span><span>FTPS</span>
      </div>
      <div
        class="about-tabs"
        role="tablist"
        aria-label="Programminformationen"
        @keydown="switchTab"
      >
        <button
          id="about-tab"
          role="tab"
          :tabindex="tab === 'about' ? 0 : -1"
          :aria-selected="tab === 'about'"
          aria-controls="about-content"
          @click="tab = 'about'"
        >
          Informationen
        </button>
        <button
          id="help-tab"
          role="tab"
          :tabindex="tab === 'help' ? 0 : -1"
          :aria-selected="tab === 'help'"
          aria-controls="help-content"
          @click="tab = 'help'"
        >
          Bedienung
        </button>
      </div>
      <div
        v-if="tab === 'about'"
        id="about-content"
        role="tabpanel"
        aria-labelledby="about-tab"
        class="about-content"
      >
        <dl class="about-facts">
          <div>
            <dt>Plattformen</dt>
            <dd>Linux, Windows & macOS</dd>
          </div>
          <div>
            <dt>Technologie</dt>
            <dd>Tauri · Vue · Rust</dd>
          </div>
        </dl>
        <p v-if="!desktop" class="about-note">
          Die Vorschau speichert keine Geheimnisse und baut keine echten Verbindungen auf.
        </p>
        <details class="about-details">
          <summary><Icon name="LockKeyhole" :size="15" />Daten & Speicherung</summary>
          <p>
            Dein lokaler Tresor ist verschlüsselt. Team-Daten werden vor dem Upload verschlüsselt;
            Zugangsdaten teilst du nur ausdrücklich. Die Kontoauswahl an Verbindungen bleibt
            persönlich.
          </p>
          <div v-if="store.vaultPath" class="about-vault">
            <code>{{ store.vaultPath }}</code
            ><button class="icon-button" title="Tresorpfad kopieren" @click="copyLocation">
              <Icon name="Copy" :size="15" />
            </button>
          </div>
          <p v-else>Der Tresorpfad ist in der Desktop-App verfügbar.</p>
        </details>
        <details class="about-details">
          <summary>Schriften & Lizenzen</summary>
          <p>
            Inter Variable und JetBrains Mono Variable sind offline enthalten, jeweils unter der SIL
            Open Font License 1.1.
          </p>
        </details>
      </div>
      <div
        v-else
        id="help-content"
        role="tabpanel"
        aria-labelledby="help-tab"
        class="about-content about-help"
      >
        <p>
          <strong>Terminal & Dateien</strong>SSH öffnet das Terminal mit optionaler Dateiansicht.
          SFTP, FTP und FTPS öffnen direkt den Dateibrowser. Klassisches FTP überträgt
          unverschlüsselt.
        </p>
        <p>
          <strong>Ordner übertragen</strong>Dateien oder ganze Ordner zwischen lokalem und
          entferntem Bereich ziehen. Vorhandene Zielordner bleiben geschützt; Verknüpfungen und
          Spezialdateien werden ausgelassen und gemeldet.
        </p>
        <p>
          <strong>sudo & Anmeldung</strong>SSH-Dateizugriff unterstützt sudo ohne Python.
          Gespeicherte Benutzerpasswörter werden bei Bedarf verwendet; OTP-Abfragen erscheinen beim
          Verbinden.
        </p>
        <div class="about-shortcuts">
          <span>Pfad eingeben</span><kbd>Strg / ⌘ L</kbd><span>Übergeordneter Ordner</span
          ><kbd>Alt ↑</kbd><span>Aktualisieren</span><kbd>F5</kbd>
        </div>
      </div>
      <div class="about-actions">
        <button class="button secondary" @click="openProject">
          <Icon name="ExternalLink" :size="15" />GitHub</button
        ><button class="button secondary" @click="releaseDownloads">Downloads</button
        ><button
          v-if="desktop"
          class="button primary"
          :disabled="updateBusy || !updater.enabled"
          @click="updates"
        >
          <Icon
            :name="updateBusy ? 'LoaderCircle' : 'RefreshCw'"
            :size="15"
            :class="{ spin: updateBusy }"
          />Updates prüfen
        </button>
      </div>
    </div>
  </Modal>
</template>
<style scoped>
.program-info {
  padding: 1.5rem;
}
.about-brand {
  display: flex;
  align-items: center;
  gap: 1rem;
}
.about-brand img {
  width: 5.142857rem;
  height: 5.142857rem;
  flex-shrink: 0;
}
.about-brand h3 {
  margin: 0 0 0.35rem;
  font-size: 2.5rem;
  letter-spacing: -0.09rem;
  font-weight: 650;
  color: var(--text-strong);
}
.about-version {
  color: var(--text-secondary);
  font-family: var(--font-mono);
  font-size: 0.928571rem;
}
.about-description {
  margin: 1rem 0 0.5rem;
  font-size: 1.285714rem;
  font-weight: 550;
  color: var(--text-strong);
}
.about-intro {
  color: var(--text-secondary);
  line-height: 1.7;
  margin: 0;
}
.about-protocols {
  display: flex;
  flex-wrap: wrap;
  gap: 0.5rem;
  margin: 0.8rem 0 1.15rem;
}
.about-protocols span {
  padding: 0.3rem 0.65rem;
  border: 1px solid var(--border);
  border-radius: 0.4rem;
  font-family: var(--font-mono);
  font-size: 0.857143rem;
  color: var(--text-secondary);
  background: var(--surface-raised);
}
.about-tabs {
  display: flex;
  gap: 1.5rem;
  border-bottom: 1px solid var(--border);
}
.about-tabs button {
  padding: 0 0 0.75rem;
  color: var(--muted);
  border-bottom: 2px solid transparent;
  font-weight: 550;
}
.about-tabs button[aria-selected='true'] {
  color: var(--text-strong);
  border-color: var(--accent);
}
.about-content {
  padding: 0.5rem 0 1rem;
}
.about-facts {
  margin: 0.3rem 0 0.8rem;
}
.about-facts > div {
  display: flex;
  justify-content: space-between;
  gap: 1rem;
  padding: 0.4rem 0;
}
.about-facts dt {
  color: var(--muted);
}
.about-facts dd {
  margin: 0;
  color: var(--text-secondary);
  text-align: right;
}
.about-details {
  border-top: 1px solid var(--border);
  color: var(--text-secondary);
}
.about-details summary {
  cursor: pointer;
  padding: 0.65rem 0;
  font-weight: 500;
}
.about-details summary svg {
  vertical-align: text-bottom;
  margin-right: 0.4rem;
}
.about-details p,
.about-help p,
.about-note {
  margin: 0 0 0.8rem;
  line-height: 1.65;
  color: var(--text-secondary);
}
.about-vault {
  display: flex;
  gap: 0.5rem;
  align-items: center;
  margin: 0 0 0.9rem;
  border: 1px solid var(--border);
  border-radius: 0.5rem;
  padding: 0.5rem 0.65rem;
}
.about-vault code {
  flex: 1;
  min-width: 0;
  overflow-wrap: anywhere;
  font-size: 0.857143rem;
}

.about-help {
  padding-top: 1rem;
}
.about-help strong {
  display: block;
  margin-bottom: 0.25rem;
  color: var(--text-strong);
}
.about-shortcuts {
  display: grid;
  grid-template-columns: 1fr auto;
  align-items: center;
  gap: 0.6rem;
  margin-top: 1rem;
  color: var(--text-secondary);
}
.about-actions {
  display: flex;
  flex-wrap: wrap;
  gap: 0.6rem;
  padding-top: 1rem;
  border-top: 1px solid var(--border);
}
.about-actions .primary {
  margin-left: auto;
}
</style>
