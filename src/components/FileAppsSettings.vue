<script setup lang="ts">
import { onMounted, ref } from 'vue';
import { invoke } from '@tauri-apps/api/core';
import { open } from '@tauri-apps/plugin-dialog';
import { desktop, notify } from '../composables/useNoryn';
import Icon from './Icon.vue';
interface Rule {
  extensions: string[];
  program: string;
  arguments: string[];
  host: boolean;
  wait: boolean;
}
interface Draft {
  extensions: string;
  program: string;
  arguments: string;
  host: boolean;
  wait: boolean;
}
const rules = ref<Draft[]>([]),
  flatpak = ref(false),
  error = ref(''),
  busy = ref(false);
onMounted(async () => {
  if (!desktop) return;
  try {
    const result = await invoke<{ rules: Rule[]; flatpak: boolean }>('file_apps_get');
    flatpak.value = result.flatpak;
    rules.value = result.rules.map((r) => ({
      ...r,
      extensions: r.extensions.join(', '),
      arguments: r.arguments.join('\n'),
    }));
  } catch (cause) {
    error.value = String(cause);
  }
});
async function choose(rule: Draft) {
  try {
    const path = await open({ title: 'Programm auswählen', directory: false, multiple: false });
    if (typeof path === 'string') rule.program = path;
  } catch (cause) {
    error.value = String(cause);
  }
}
async function save() {
  if (!desktop) {
    error.value = 'Diese Einstellung ist nur in der Desktop-App verfügbar.';
    return;
  }
  busy.value = true;
  error.value = '';
  try {
    await invoke('file_apps_save', {
      rules: rules.value.map((r) => ({
        ...r,
        extensions: r.extensions
          .split(',')
          .map((s) => s.trim().replace(/^\./, '').toLowerCase())
          .filter(Boolean),
        arguments: r.arguments.split('\n').filter((s) => s.length),
      })),
    });
    notify('Programmeinstellungen gespeichert');
  } catch (cause) {
    error.value = String(cause);
  } finally {
    busy.value = false;
  }
}
</script>
<template>
  <section class="settings-section" aria-labelledby="file-apps-heading">
    <div class="settings-section-head">
      <div>
        <h2 id="file-apps-heading">Programme für Dateien</h2>
        <p>
          Persönliche Zuordnungen je Dateiendung. Ohne Zuordnung gilt das Standardprogramm des
          Betriebssystems.
        </p>
      </div>
      <button
        class="button secondary"
        :disabled="busy || rules.length >= 32"
        @click="
          rules.push({ extensions: '', program: '', arguments: '', host: false, wait: false })
        "
      >
        <Icon name="Plus" :size="15" />Zuordnung
      </button>
    </div>
    <div v-for="(rule, i) in rules" :key="i" class="editor-rule form-grid">
      <label
        >Dateiendungen<input v-model="rule.extensions" placeholder="txt, md, sh" :disabled="busy"
      /></label>
      <label
        >Programmpfad
        <div class="program-path">
          <input v-model="rule.program" placeholder="/usr/bin/kate" :disabled="busy" /><button
            class="button secondary"
            :disabled="busy"
            @click="choose(rule)"
          >
            Auswählen …
          </button>
        </div></label
      >
      <label class="full"
        >Argumente (ein Argument je Zeile)<textarea
          v-model="rule.arguments"
          :disabled="busy"
          rows="2"
          placeholder="--wait"
        /><span class="muted"
          >{file} als eigenes Argument einsetzen; sonst wird der Dateipfad angehängt. Keine
          Shell-Auswertung.</span
        ></label
      >
      <label class="checkbox-row full"
        ><input v-model="rule.wait" type="checkbox" :disabled="busy" />Programm meldet den
        Dokumentabschluss verlässlich (z. B. Kate mit --block --new)</label
      >
      <p class="muted full">
        Nur aktivieren, wenn der Prozess bis zum Schließen des Dokuments wartet. Sonst bleibt
        „Bearbeitung beenden“ für den Upload erforderlich.
      </p>
      <label v-if="flatpak" class="checkbox-row full"
        ><input v-model="rule.host" type="checkbox" :disabled="busy" />Programm außerhalb der
        Flatpak-Sandbox starten</label
      >
      <p v-if="flatpak && rule.host" class="muted full">
        Benötigt ausdrücklich freigegebenen D-Bus-Zugriff auf org.freedesktop.Flatpak (z. B. über
        Flatseal). Diese Berechtigung erlaubt Host-Programme; Noryn vergibt sie nicht automatisch.
        Den absoluten Host-Pfad angeben. Standardprogramme über das Portal brauchen diese Freigabe
        nicht.
      </p>
      <button class="button secondary danger" :disabled="busy" @click="rules.splice(i, 1)">
        <Icon name="Trash2" :size="15" />Zuordnung entfernen
      </button>
    </div>
    <p v-if="error" class="inline-error" role="alert">{{ error }}</p>
    <button class="button secondary" :disabled="busy" @click="save">
      {{ busy ? 'Wird gespeichert …' : 'Programmzuordnungen speichern' }}
    </button>
  </section>
</template>
<style scoped>
.form-grid {
  display: grid;
  grid-template-columns: repeat(2, minmax(0, 1fr));
  gap: 1rem;
}
.full {
  grid-column: 1 / -1;
}
@media (max-width: 700px) {
  .form-grid {
    grid-template-columns: 1fr;
  }
}
.editor-rule {
  border: 1px solid var(--border);
  border-radius: 10px;
  padding: 1rem;
  margin: 1rem 0;
}
.program-path {
  display: flex;
  gap: 0.5rem;
}
.program-path input {
  min-width: 0;
  flex: 1;
}
</style>
