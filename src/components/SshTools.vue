<script setup lang="ts">
import { errorText, t as tr } from '../i18n';

import { computed, onBeforeUnmount, onMounted, reactive, ref, watch } from 'vue';
import { invoke } from '@tauri-apps/api/core';
import type { ClientSession, ForwardInfo, ListeningPort } from '../types';
import Icon from './Icon.vue';
import SystemView from './SystemView.vue';
import SelectMenu from './SelectMenu.vue';
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
const listeners = ref<ListeningPort[]>(props.session.health?.ports || []);
const portsBusy = ref(false),
  portsError = ref(''),
  probing = ref('');
const probeResults = reactive<Record<string, { ok: boolean; text: string }>>({});
const forwardForm = ref<HTMLElement>();
const webScheme = ref<'http' | 'https'>('http');
async function openWeb(forwardId: string) {
  if (!active.value) return;
  if (props.session.preview) {
    error.value = tr('Browser-Vorschau: Es wird kein echter Webdienst geöffnet.');
    return;
  }
  try {
    await invoke('ssh_forward_open', { id: props.session.id, forwardId, scheme: webScheme.value });
  } catch (e) {
    error.value = String(e);
  }
}
let portsRefreshed = false;
watch(
  () => props.session.health,
  (health) => {
    if (!portsRefreshed && health) listeners.value = health.ports;
  },
  { immediate: true },
);
async function refreshPorts() {
  if (!active.value || portsBusy.value) return;
  portsBusy.value = true;
  portsError.value = '';
  try {
    const ports = props.session.preview
      ? props.session.health?.ports || []
      : await invoke<ListeningPort[]>('ssh_listening_ports', { id: props.session.id });
    if (!disposed) {
      listeners.value = ports;
      portsRefreshed = true;
    }
  } catch (e) {
    portsError.value = String(e);
  } finally {
    portsBusy.value = false;
  }
}
function choosePort(port: ListeningPort) {
  if (port.protocol !== 'tcp' || !props.session.connected) return;
  Object.assign(form, {
    remoteHost: port.forwardHost,
    remotePort: port.port,
    localPort: port.port,
  });
  forwardForm.value?.scrollIntoView({ behavior: 'smooth', block: 'center' });
  forwardForm.value?.querySelector('input')?.focus();
}
async function probe(id: string) {
  if (probing.value || !props.session.connected) return;
  probing.value = id;
  try {
    const text = props.session.preview
      ? tr('Vorschau: TCP-Ziel erreichbar. Es wurden keine echten Pakete gesendet.')
      : await invoke<string>('ssh_forward_probe', { id: props.session.id, forwardId: id });
    if (!disposed) probeResults[id] = { ok: true, text };
  } catch (e) {
    if (!disposed) probeResults[id] = { ok: false, text: String(e) };
  } finally {
    probing.value = '';
  }
}
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
    error.value = tr('Adresse konnte nicht kopiert werden.');
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
          <span class="eyebrow">{{ tr('SSH ·') }} {{ session.host }}</span>
          <h2>{{ tr('Lokale Weiterleitungen') }}</h2>
          <p>{{ tr('Erreiche Dienste des Servers direkt auf deinem Computer.') }}</p>
        </div>
        <button class="button secondary small" :disabled="busy || !active" @click="refresh">
          <Icon name="RefreshCw" :size="14" :class="{ spin: busy }" />{{ tr('Aktualisieren') }}
        </button>
      </div>
      <p v-if="!session.connected" class="tools-notice">
        {{ tr('Sitzung beendet. Weiterleitungen sind geschlossen.') }}
      </p>
      <p v-if="error" class="error-message">{{ errorText(error) }}</p>

      <form ref="forwardForm" class="forward-form" @submit.prevent="start">
        <label
          >{{ tr('Lokaler Port')
          }}<input
            v-model.number="form.localPort"
            type="number"
            min="0"
            max="65535"
            required
          /><small>{{ tr('127.0.0.1 · 0 = freien Port wählen') }}</small></label
        ><Icon name="ArrowRight" :size="20" />
        <label
          >{{ tr('Ziel auf dem Server')
          }}<input
            v-model.trim="form.remoteHost"
            required
            maxlength="253"
            placeholder="127.0.0.1"
          /><small>{{ tr('Aus Sicht des SSH-Servers') }}</small></label
        >
        <label
          >{{ tr('Zielport')
          }}<input
            v-model.number="form.remotePort"
            type="number"
            min="1"
            max="65535"
            required
          /><small>{{ tr('z. B. PostgreSQL: 5432') }}</small></label
        >
        <button class="button primary" :disabled="busy || !active">
          <Icon name="Plus" :size="16" />{{ tr('Weiterleiten') }}
        </button>
      </form>
      <div v-if="!forwards.length" class="tools-empty">
        <Icon name="Link" :size="30" />
        <h3>{{ tr('Ein kurzer Weg zu deinem Server.') }}</h3>
        <p>{{ tr('Zum Beispiel PostgreSQL auf Port 5432 oder ein internes Webinterface.') }}</p>
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
          <SelectMenu
            v-model="webScheme"
            :options="[
              { value: 'http', label: 'HTTP' },
              { value: 'https', label: 'HTTPS' },
            ]"
            :label="tr('Web-Protokoll')"
          />
          <button class="button secondary small" :disabled="!active" @click="openWeb(forward.id)">
            <Icon name="Globe" :size="14" />{{ tr('Im Browser öffnen') }}
          </button>
          <button
            class="button secondary small"
            :disabled="!!probing || !active"
            @click="probe(forward.id)"
          >
            <Icon
              :name="probing === forward.id ? 'LoaderCircle' : 'Activity'"
              :class="{ spin: probing === forward.id }"
              :size="14"
            />{{ tr('TCP-Ziel testen') }}
          </button>
          <span>{{ forward.activeConnections }} {{ tr('Verbindungen') }}</span
          ><span>↑ {{ bytes(forward.uploaded) }} · ↓ {{ bytes(forward.downloaded) }}</span
          ><button
            class="icon-button"
            :aria-label="tr('Lokale Adresse kopieren')"
            @click="copy(forward.localPort)"
          >
            <Icon name="Copy" :size="15" /></button
          ><button
            class="button secondary small"
            :disabled="busy || !active"
            @click="stop(forward.id)"
          >
            <Icon name="X" :size="14" />{{ tr('Beenden') }}
          </button>
        </div>
        <p v-if="forward.lastError" class="error-message">{{ forward.lastError }}</p>
        <p
          v-if="probeResults[forward.id]"
          :class="probeResults[forward.id].ok ? 'tools-notice' : 'error-message'"
          role="status"
        >
          {{ probeResults[forward.id].text }}
        </p>
      </article>
      <section class="listener-section">
        <div class="tools-heading">
          <div>
            <h3>{{ tr('Offene Ports auf dem Server') }}</h3>
            <p>
              {{
                tr(
                  'Listener aus ss · einmal beim Login gelesen. Prozessnamen sind je nach Benutzerrechten verfügbar.',
                )
              }}
            </p>
          </div>
          <button
            class="button secondary small"
            :disabled="portsBusy || !active"
            @click="refreshPorts"
          >
            <Icon name="RefreshCw" :size="14" :class="{ spin: portsBusy }" />{{
              tr('Ports neu lesen')
            }}
          </button>
        </div>
        <p v-if="portsError" class="error-message" role="alert">{{ portsError }}</p>
        <p v-if="!listeners.length" class="muted">
          {{
            session.healthLoading
              ? tr('Portliste wird beim Login gelesen …')
              : session.health?.portsAvailable
                ? tr('Keine Listener in der gelesenen Liste.')
                : tr('Noch keine lesbare Portliste. Mit „Ports neu lesen“ erneut versuchen.')
          }}
        </p>
        <div
          v-for="port in listeners"
          :key="`${port.protocol}-${port.address}-${port.port}`"
          class="listener-row"
        >
          <span class="small-badge">{{ port.protocol.toUpperCase() }}</span
          ><strong class="mono">{{ port.address }}:{{ port.port }}</strong
          ><span class="listener-process" :title="port.process">{{
            port.process || tr('Prozess nicht lesbar')
          }}</span
          ><button
            class="button secondary small"
            :disabled="port.protocol !== 'tcp' || !active"
            :title="
              port.protocol === 'udp'
                ? tr('SSH-Weiterleitungen unterstützen hier TCP.')
                : tr('Adresse und Port ins Formular übernehmen')
            "
            @click="choosePort(port)"
          >
            {{ port.protocol === 'udp' ? tr('Nur TCP weiterleitbar') : tr('Weiterleiten …') }}
          </button>
        </div>
      </section>
      <p class="tools-footnote">
        {{ tr('Nur auf 127.0.0.1 erreichbar. Alle Weiterleitungen enden mit der SSH-Sitzung.') }}
      </p>
    </div>
  </div>
</template>
