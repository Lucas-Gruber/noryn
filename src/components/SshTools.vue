<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, reactive, ref, watch } from 'vue';
import { invoke } from '@tauri-apps/api/core';
import type { ClientSession, ForwardInfo, SystemSection, SystemSnapshot } from '../types';
import Icon from './Icon.vue';
const props = defineProps<{
  session: ClientSession;
  visible: boolean;
  mode: 'forwards' | 'system';
}>();
const forwards = ref<ForwardInfo[]>([]);
const form = reactive({ remoteHost: '127.0.0.1', remotePort: 5432, localPort: 5432 });
const section = ref<SystemSection>('processes');
const snapshots = reactive<Partial<Record<SystemSection, SystemSnapshot>>>({});
const snapshot = computed(() => snapshots[section.value]);
const busy = ref(false);
const error = ref('');
const auto = ref(false);
const search = ref('');
const foreground = ref(!document.hidden);
let timer: ReturnType<typeof setTimeout> | undefined;
let generation = 0;
const active = computed(() => props.visible && props.session.connected && foreground.value);
const processes = computed(
  () =>
    snapshot.value?.processes.filter((p) =>
      `${p.pid} ${p.name} ${p.user}`.toLocaleLowerCase().includes(search.value.toLocaleLowerCase()),
    ) || [],
);
const services = computed(
  () =>
    snapshot.value?.services.filter((s) =>
      `${s.name} ${s.description}`.toLocaleLowerCase().includes(search.value.toLocaleLowerCase()),
    ) || [],
);
function bytes(n: number) {
  return n >= 1073741824
    ? `${(n / 1073741824).toFixed(1)} GiB`
    : n >= 1048576
      ? `${(n / 1048576).toFixed(1)} MiB`
      : `${(n / 1024).toFixed(1)} KiB`;
}
function preview(): SystemSnapshot {
  return {
    section: section.value,
    capturedAt: Date.now() / 1000,
    message: null,
    processes: [
      {
        pid: 1432,
        parentPid: 1,
        user: 'postgres',
        cpu: 1.7,
        memoryPercent: 2.2,
        memory: 184549376,
        name: 'postgres',
      },
      {
        pid: 896,
        parentPid: 1,
        user: 'www-data',
        cpu: 0.3,
        memoryPercent: 0.4,
        memory: 33554432,
        name: 'nginx',
      },
      {
        pid: 2150,
        parentPid: 2131,
        user: props.session.username,
        cpu: 0,
        memoryPercent: 0.1,
        memory: 8388608,
        name: 'bash',
      },
    ],
    services: [
      {
        name: 'postgresql.service',
        load: 'loaded',
        active: 'active',
        state: 'running',
        description: 'PostgreSQL database server',
      },
      {
        name: 'nginx.service',
        load: 'loaded',
        active: 'active',
        state: 'running',
        description: 'A high performance web server',
      },
    ],
    hardware: {
      hostname: props.session.name,
      os: 'Debian GNU/Linux 13',
      kernel: 'Linux 6.12',
      architecture: 'x86_64',
      cpuModel: 'AMD EPYC',
      logicalCpus: 4,
      memoryTotal: 8589934592,
      memoryAvailable: 6442450944,
      swapTotal: 2147483648,
      swapFree: 2147483648,
      uptime: 1067220,
      load: [0.14, 0.22, 0.19],
      disks: [
        {
          name: '/dev/vda1',
          total: 107374182400,
          used: 26843545600,
          available: 75161927680,
          mount: '/',
        },
      ],
    },
  };
}
function schedule() {
  clearTimeout(timer);
  if (active.value && (props.mode === 'forwards' || (auto.value && section.value !== 'hardware')))
    timer = setTimeout(() => void refresh(), 5000);
}
async function refresh() {
  clearTimeout(timer);
  if (!active.value || busy.value) return;
  const request = ++generation;
  const requestedSection = section.value;
  const requestedMode = props.mode;
  busy.value = true;
  error.value = '';
  try {
    if (requestedMode === 'forwards') {
      const result = props.session.preview
        ? forwards.value
        : await invoke<ForwardInfo[]>('ssh_forward_list', { id: props.session.id });
      if (request === generation) forwards.value = result;
    } else {
      const result = props.session.preview
        ? preview()
        : await invoke<SystemSnapshot>('ssh_system_snapshot', {
            id: props.session.id,
            section: requestedSection,
          });
      if (request === generation) snapshots[requestedSection] = result;
    }
  } catch (e) {
    if (request === generation) error.value = String(e);
  } finally {
    busy.value = false;
    if (active.value && (requestedMode !== props.mode || requestedSection !== section.value))
      void refresh();
    else schedule();
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
    error.value = 'Adresse konnte nicht in die Zwischenablage kopiert werden.';
  }
}
function visibility() {
  foreground.value = !document.hidden;
}
watch(
  [active, () => props.mode, section],
  () => {
    clearTimeout(timer);
    if (active.value) void refresh();
  },
  { immediate: true },
);
watch(auto, schedule);
onMounted(() => document.addEventListener('visibilitychange', visibility));
onBeforeUnmount(() => {
  ++generation;
  clearTimeout(timer);
  document.removeEventListener('visibilitychange', visibility);
});
</script>
<template>
  <div class="ssh-tools">
    <div class="tools-heading">
      <div>
        <span class="eyebrow">SSH · {{ session.host }}</span>
        <h2>{{ mode === 'forwards' ? 'Lokale Weiterleitungen' : 'Systemübersicht' }}</h2>
        <p>
          {{
            mode === 'forwards'
              ? 'Erreiche Dienste des Servers direkt auf deinem Computer.'
              : 'Kleine Abfragen über deine bestehende Verbindung. Ohne zusätzlichen Agenten.'
          }}
        </p>
      </div>
      <button class="button secondary small" :disabled="busy || !active" @click="refresh">
        <Icon name="RefreshCw" :size="14" :class="{ spin: busy }" />Aktualisieren
      </button>
    </div>
    <p v-if="!session.connected" class="tools-notice">
      Sitzung beendet. Weiterleitungen sind geschlossen.
    </p>
    <p v-if="error" class="error-message" role="alert">{{ error }}</p>
    <template v-if="mode === 'forwards'">
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
    </template>
    <template v-else>
      <div class="system-tabs">
        <button
          v-for="tab in ['processes', 'services', 'hardware'] as const"
          :key="tab"
          :class="{ selected: section === tab }"
          @click="
            section = tab;
            search = '';
          "
        >
          {{
            tab === 'processes' ? 'Prozesse' : tab === 'services' ? 'Dienste' : 'Hardware'
          }}</button
        ><label v-if="section !== 'hardware'" class="checkbox-row"
          ><input v-model="auto" type="checkbox" />Automatisch · 5 s</label
        >
      </div>
      <div class="system-filter">
        <div v-if="section !== 'hardware'" class="search-field">
          <Icon name="Search" :size="14" /><input
            v-model="search"
            placeholder="In der Ansicht suchen …"
          />
        </div>
        <span v-if="snapshot"
          >Stand {{ new Date(snapshot.capturedAt * 1000).toLocaleTimeString() }}</span
        >
      </div>
      <p v-if="snapshot?.message" class="tools-notice">{{ snapshot.message }}</p>
      <div v-if="section === 'processes' && snapshot" class="tools-table-wrap">
        <table class="tools-table">
          <thead>
            <tr>
              <th>Prozess</th>
              <th>PID</th>
              <th>Benutzer</th>
              <th title="Mittlere CPU-Nutzung über die gesamte Prozesslaufzeit">CPU Ø</th>
              <th>RAM</th>
            </tr>
          </thead>
          <tbody>
            <tr v-for="p in processes" :key="p.pid">
              <td class="mono">{{ p.name }}</td>
              <td class="mono">{{ p.pid }}</td>
              <td>{{ p.user }}</td>
              <td>{{ p.cpu.toFixed(1) }} %</td>
              <td>{{ bytes(p.memory) }}</td>
            </tr>
          </tbody>
        </table>
        <p class="tools-footnote">
          Bis zu 100 Prozesse, nach mittlerer CPU-Nutzung sortiert. CPU Ø gilt seit Prozessstart.
        </p>
      </div>
      <div v-if="section === 'services' && snapshot && !snapshot.message" class="tools-table-wrap">
        <table class="tools-table">
          <thead>
            <tr>
              <th>Dienst</th>
              <th>Status</th>
              <th>Beschreibung</th>
            </tr>
          </thead>
          <tbody>
            <tr v-for="s in services" :key="s.name">
              <td class="mono">{{ s.name }}</td>
              <td>
                <span
                  class="service-state"
                  :class="{ running: s.active === 'active', failed: s.active === 'failed' }"
                  >{{ s.state }}</span
                >
              </td>
              <td>{{ s.description }}</td>
            </tr>
          </tbody>
        </table>
        <p class="tools-footnote">
          Bis zu 400 geladene systemd-Dienste. Ansicht ohne Verwaltungsaktionen.
        </p>
      </div>
      <div v-if="section === 'hardware' && snapshot?.hardware" class="hardware-view">
        <div class="hardware-cards">
          <article>
            <Icon name="Server" :size="20" /><small>System</small
            ><strong>{{ snapshot.hardware.hostname }}</strong>
            <p>
              {{ snapshot.hardware.os }}<br />{{ snapshot.hardware.kernel }} ·
              {{ snapshot.hardware.architecture }}
            </p>
          </article>
          <article>
            <Icon name="Cpu" :size="20" /><small>Prozessor</small
            ><strong>{{ snapshot.hardware.logicalCpus }} logische CPUs</strong>
            <p>
              {{ snapshot.hardware.cpuModel }}<br />Last:
              {{ snapshot.hardware.load.map((n) => n.toFixed(2)).join(' / ') }}
            </p>
          </article>
          <article>
            <Icon name="MemoryStick" :size="20" /><small>Arbeitsspeicher</small
            ><strong
              >{{ bytes(snapshot.hardware.memoryTotal - snapshot.hardware.memoryAvailable) }} /
              {{ bytes(snapshot.hardware.memoryTotal) }}</strong
            >
            <p>
              {{ bytes(snapshot.hardware.memoryAvailable) }} verfügbar<br />Swap:
              {{ bytes(snapshot.hardware.swapTotal - snapshot.hardware.swapFree) }} /
              {{ bytes(snapshot.hardware.swapTotal) }}
            </p>
          </article>
        </div>
        <p class="tools-footnote">
          Laufzeit: {{ Math.floor(snapshot.hardware.uptime / 86400) }} Tage,
          {{ Math.floor((snapshot.hardware.uptime % 86400) / 3600) }} Stunden
        </p>
        <table class="tools-table">
          <thead>
            <tr>
              <th>Datenträger</th>
              <th>Eingehängt unter</th>
              <th>Belegt</th>
              <th>Verfügbar</th>
            </tr>
          </thead>
          <tbody>
            <tr v-for="disk in snapshot.hardware.disks" :key="disk.name + disk.mount">
              <td class="mono">{{ disk.name }}</td>
              <td class="mono">{{ disk.mount }}</td>
              <td>{{ bytes(disk.used) }} / {{ bytes(disk.total) }}</td>
              <td>{{ bytes(disk.available) }}</td>
            </tr>
          </tbody>
        </table>
      </div>
    </template>
  </div>
</template>
