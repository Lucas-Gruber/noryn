<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, reactive, ref, watch } from 'vue';
import { invoke } from '@tauri-apps/api/core';
import type { ClientSession, ForwardInfo } from '../types';
import Icon from './Icon.vue';
import SystemView from './SystemView.vue';
const props = defineProps<{
  session: ClientSession;
  visible: boolean;
  mode: 'forwards' | 'system';
}>();
const forwards = ref<ForwardInfo[]>([]),
  busy = ref(false),
  error = ref(''),
  foreground = ref(!document.hidden);
const form = reactive({ remoteHost: '127.0.0.1', remotePort: 5432, localPort: 5432 });
const active = computed(
  () => props.visible && props.mode === 'forwards' && props.session.connected && foreground.value,
);
let timer: ReturnType<typeof setTimeout> | undefined,
  disposed = false;
function bytes(n: number) {
  return n >= 1048576 ? `${(n / 1048576).toFixed(1)} MiB` : `${(n / 1024).toFixed(1)} KiB`;
}
function schedule() {
  clearTimeout(timer);
  if (active.value) timer = setTimeout(() => void refresh(), 5000);
}
async function refresh() {
  clearTimeout(timer);
  if (!active.value || busy.value) return;
  busy.value = true;
  error.value = '';
  try {
    if (!props.session.preview) {
      const result = await invoke<ForwardInfo[]>('ssh_forward_list', { id: props.session.id });
      if (!disposed) forwards.value = result;
    }
  } catch (e) {
    error.value = String(e);
  } finally {
    busy.value = false;
    schedule();
  }
}
async function start() {
  if (busy.value || !active.value) return;
  busy.value = true;
  error.value = '';
  try {
    const entry = props.session.preview
      ? {
          id: crypto.randomUUID(),
          ...form,
          localPort: form.localPort || 49152,
          activeConnections: 0,
          uploaded: 0,
          downloaded: 0,
          lastError: null,
        }
      : await invoke<ForwardInfo>('ssh_forward_start', { id: props.session.id, ...form });
    forwards.value = [...forwards.value, entry];
  } catch (e) {
    error.value = String(e);
  } finally {
    busy.value = false;
    schedule();
  }
}
async function stop(id: string) {
  if (busy.value) return;
  busy.value = true;
  error.value = '';
  try {
    if (!props.session.preview)
      await invoke('ssh_forward_stop', { id: props.session.id, forwardId: id });
    forwards.value = forwards.value.filter((f) => f.id !== id);
  } catch (e) {
    error.value = String(e);
  } finally {
    busy.value = false;
    schedule();
  }
}
async function copy(port: number) {
  try {
    await navigator.clipboard.writeText(`127.0.0.1:${port}`);
  } catch {
    error.value = 'Adresse konnte nicht kopiert werden.';
  }
}
function visibility() {
  foreground.value = !document.hidden;
}
watch(
  active,
  () => {
    clearTimeout(timer);
    if (active.value) void refresh();
  },
  { immediate: true },
);
onMounted(() => document.addEventListener('visibilitychange', visibility));
onBeforeUnmount(() => {
  disposed = true;
  clearTimeout(timer);
  document.removeEventListener('visibilitychange', visibility);
});
</script>
<template>
  <div class="ssh-tools-host">
    <SystemView v-if="mode === 'system'" :session="session" :visible="visible" />
    <div v-else class="ssh-tools">
      <div class="tools-heading">
        <div>
          <span class="eyebrow">SSH · {{ session.host }}</span>
          <h2>Lokale Weiterleitungen</h2>
          <p>Erreiche Dienste des Servers direkt auf deinem Computer.</p>
        </div>
        <button class="button secondary small" :disabled="busy || !active" @click="refresh">
          <Icon name="RefreshCw" :size="14" :class="{ spin: busy }" />Aktualisieren
        </button>
      </div>
      <p v-if="!session.connected" class="tools-notice">
        Sitzung beendet. Weiterleitungen sind geschlossen.
      </p>
      <p v-if="error" class="error-message">{{ error }}</p>

      <form class="forward-form" @submit.prevent="start">
        <label
          >Lokaler Port<input
            v-model.number="form.localPort"
            type="number"
            min="0"
            max="65535"
            required
          /><small>127.0.0.1 · 0 = freien Port wählen</small></label
        ><Icon name="ArrowRight" :size="20" />
        <label
          >Ziel auf dem Server<input
            v-model.trim="form.remoteHost"
            required
            maxlength="253"
            placeholder="127.0.0.1"
          /><small>Aus Sicht des SSH-Servers</small></label
        >
        <label
          >Zielport<input
            v-model.number="form.remotePort"
            type="number"
            min="1"
            max="65535"
            required
          /><small>z. B. PostgreSQL: 5432</small></label
        >
        <button class="button primary" :disabled="busy || !active">
          <Icon name="Plus" :size="16" />Weiterleiten
        </button>
      </form>
      <div v-if="!forwards.length" class="tools-empty">
        <Icon name="Link" :size="30" />
        <h3>Ein kurzer Weg zu deinem Server.</h3>
        <p>Zum Beispiel PostgreSQL auf Port 5432 oder ein internes Webinterface.</p>
      </div>
      <article v-for="forward in forwards" :key="forward.id" class="forward-card">
        <div class="forward-route">
          <span
            class="status-dot"
            :class="{ amber: !!forward.lastError || !session.connected }"
          ></span
          ><strong class="mono">127.0.0.1:{{ forward.localPort }}</strong
          ><Icon name="ArrowRight" :size="16" /><span class="mono"
            >{{ forward.remoteHost }}:{{ forward.remotePort }}</span
          >
        </div>
        <div class="forward-stats">
          <span>{{ forward.activeConnections }} Verbindungen</span
          ><span>↑ {{ bytes(forward.uploaded) }} · ↓ {{ bytes(forward.downloaded) }}</span
          ><button
            class="icon-button"
            aria-label="Lokale Adresse kopieren"
            @click="copy(forward.localPort)"
          >
            <Icon name="Copy" :size="15" /></button
          ><button
            class="button secondary small"
            :disabled="busy || !active"
            @click="stop(forward.id)"
          >
            <Icon name="X" :size="14" />Beenden
          </button>
        </div>
        <p v-if="forward.lastError" class="error-message">{{ forward.lastError }}</p>
      </article>
      <p class="tools-footnote">
        Nur auf 127.0.0.1 erreichbar. Alle Weiterleitungen enden mit der SSH-Sitzung.
      </p>
    </div>
  </div>
</template>
