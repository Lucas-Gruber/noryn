<script setup lang="ts">
import { ref } from 'vue';
import {
  appearance,
  resetAppearance,
  storageAvailable,
  textSizes,
  themes,
  type TextSize,
  type ThemeId,
} from '../composables/useAppearance';
import Icon from './Icon.vue';
import VaultBackup from './VaultBackup.vue';
import FileAppsSettings from './FileAppsSettings.vue';
import UpdateSettings from './UpdateSettings.vue';
import { updater, installBlocked } from '../composables/useUpdater';
defineEmits<{ showInfo: []; resetVault: [] }>();
const preview = ref('Production Web Server');
function radioKey<T extends string>(
  event: KeyboardEvent,
  current: T,
  values: readonly T[],
  change: (value: T) => void,
) {
  if (!['ArrowLeft', 'ArrowRight', 'ArrowUp', 'ArrowDown', 'Home', 'End'].includes(event.key))
    return;
  event.preventDefault();
  const index =
    event.key === 'Home'
      ? 0
      : event.key === 'End'
        ? values.length - 1
        : (values.indexOf(current) +
            (['ArrowLeft', 'ArrowUp'].includes(event.key) ? -1 : 1) +
            values.length) %
          values.length;
  change(values[index]!);
  (event.currentTarget as HTMLElement)
    .closest('[role="radiogroup"]')
    ?.querySelectorAll<HTMLButtonElement>('[role="radio"]')
    [index]?.focus();
}
function themeKey(event: KeyboardEvent, current: ThemeId) {
  radioKey(
    event,
    current,
    themes.map((theme) => theme.id),
    (value) => (appearance.theme = value),
  );
}
function sizeKey(event: KeyboardEvent, current: TextSize) {
  radioKey(
    event,
    current,
    textSizes.map((size) => size.id),
    (value) => (appearance.textSize = value),
  );
}
</script>
<template>
  <div class="appearance-settings">
    <div class="page-head">
      <div>
        <div class="eyebrow">AUF DIESEM GERÄT</div>
        <h1>Einstellungen<span class="title-dot">.</span></h1>
        <p>Darstellung, Updates und verschlüsselte Sicherungen.</p>
      </div>
      <button class="button secondary" @click="resetAppearance">
        <Icon name="RefreshCw" :size="16" />Darstellung zurücksetzen
      </button>
    </div>
    <section class="settings-section" aria-labelledby="theme-heading">
      <div class="settings-section-head">
        <div>
          <h2 id="theme-heading">Theme</h2>
          <p>Farben für Oberfläche, Dialoge und Terminal.</p>
        </div>
        <span class="settings-autosave"><Icon name="Check" :size="14" />Sofort aktiv</span>
      </div>
      <div class="theme-options" role="radiogroup" aria-label="Theme auswählen">
        <button
          v-for="theme in themes"
          :key="theme.id"
          class="theme-option"
          :class="{ selected: appearance.theme === theme.id }"
          role="radio"
          :aria-label="theme.name"
          :aria-checked="appearance.theme === theme.id"
          :tabindex="appearance.theme === theme.id ? 0 : -1"
          @click="appearance.theme = theme.id"
          @keydown="themeKey($event, theme.id)"
        >
          <span
            class="theme-miniature"
            aria-hidden="true"
            :style="{
              '--preview-bg': theme.background,
              '--preview-panel': theme.panel,
              '--preview-accent': theme.accent,
            }"
            ><span class="theme-mini-sidebar"><i></i><i></i><i></i></span
            ><span class="theme-mini-main"><i></i><i></i><i></i></span
          ></span>
          <span class="theme-option-title"
            >{{ theme.name
            }}<Icon v-if="appearance.theme === theme.id" name="CheckCircle2" :size="17" /></span
          ><span class="theme-option-description">{{ theme.description }}</span>
        </button>
      </div>
    </section>
    <section class="settings-section" aria-labelledby="text-size-heading">
      <div class="settings-section-head">
        <div>
          <h2 id="text-size-heading">Textgröße</h2>
          <p>Eine gemeinsame Basis für Texte, Eingabefelder und Terminal.</p>
        </div>
      </div>
      <div class="text-size-options" role="radiogroup" aria-label="Basis-Textgröße auswählen">
        <button
          v-for="size in textSizes"
          :key="size.id"
          role="radio"
          :aria-label="`${size.name} · ${size.pixels} Pixel`"
          :aria-checked="appearance.textSize === size.id"
          :tabindex="appearance.textSize === size.id ? 0 : -1"
          :class="{ selected: appearance.textSize === size.id }"
          @click="appearance.textSize = size.id"
          @keydown="sizeKey($event, size.id)"
        >
          <strong>{{ size.name }}</strong
          ><span>{{ size.pixels }} px</span>
        </button>
      </div>
      <div class="appearance-preview">
        <div>
          <span class="eyebrow">LIVE-VORSCHAU</span>
          <h3>Gut lesbar, auch im Detail.</h3>
          <p>Inter für die Oberfläche. JetBrains Mono für Terminal, Pfade und technische Werte.</p>
          <label>Name<input v-model="preview" aria-label="Vorschau der Eingabeschrift" /></label
          ><span class="preview-detail"
            ><Icon name="Server" :size="15" />{{ preview || 'Production Web Server'
            }}<code>10.20.1.10:22</code></span
          >
        </div>
        <div class="settings-terminal">
          <div><Icon name="Terminal" :size="15" />Terminal-Vorschau</div>
          <pre><span class="ansi-green">lucas</span><span class="ansi-muted">@</span><span class="ansi-blue">server</span> <span class="ansi-cyan">~</span>
<span class="ansi-magenta">❯</span> ls -la
<span class="ansi-blue">drwxr-xr-x</span> <span class="ansi-cyan">projects/</span>
<span class="ansi-yellow">-rw-r--r--</span> deploy.toml
<span class="ansi-green">✓ Verbindung bereit</span></pre>
        </div>
      </div>
    </section>
    <p class="appearance-storage">
      <Icon name="LockKeyhole" :size="15" />{{
        storageAvailable
          ? 'Automatisch auf diesem Gerät gespeichert. Diese Auswahl wird nicht mit Teams synchronisiert.'
          : 'Speichern ist hier blockiert. Deine Auswahl gilt für die aktuelle Sitzung.'
      }}
    </p>
    <UpdateSettings />
    <section class="settings-section" aria-labelledby="program-heading">
      <div class="settings-section-head">
        <div>
          <h2 id="program-heading">Über Noryn.</h2>
          <p>Version {{ updater.version }} · Dein Remote Workspace.</p>
        </div>
        <button class="button secondary" @click="$emit('showInfo')">
          <Icon name="Info" :size="16" />Programminfo
        </button>
      </div>
    </section>
    <FileAppsSettings />
    <VaultBackup />
    <section class="settings-section" aria-labelledby="vault-reset-heading">
      <div class="settings-section-head">
        <div>
          <h2 id="vault-reset-heading">Lokaler Tresor</h2>
          <p>Mit einem neuen Passwort und einem leeren Tresor neu anfangen.</p>
        </div>
        <button
          class="button secondary danger"
          :disabled="installBlocked"
          @click="$emit('resetVault')"
        >
          <Icon name="Trash2" :size="16" />Sperren & zurücksetzen …
        </button>
      </div>
      <p class="muted">
        Der nächste Schritt zeigt dir die Folgen und verlangt eine ausdrückliche Bestätigung. Daten
        auf dem Team-Server und exportierte Backups bleiben erhalten.
      </p>
      <p v-if="installBlocked" class="muted">
        Bitte zuerst Verbindungen und Anmeldungen schließen und laufende Abgleiche beenden.
      </p>
    </section>
  </div>
</template>
