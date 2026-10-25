<script setup lang="ts">
import { errorText, t as tr, formatLocale } from '../i18n';

import { computed, onBeforeUnmount, onMounted, ref, watch } from 'vue';
import { invoke } from '@tauri-apps/api/core';
import type {
  ClientSession,
  SystemReply,
  SystemRequest,
  SystemSection,
  SystemSnapshot,
} from '../types';
import { notify } from '../composables/useNoryn';
import Icon from './Icon.vue';
import Modal from './Modal.vue';
import HardwareView from './HardwareView.vue';
import RemoteActionDialog from './RemoteActionDialog.vue';
const props = defineProps<{ session: ClientSession; visible: boolean }>();
const previewStarted = Date.now() / 1000;
const section = ref<SystemSection>('processes'),
  snapshot = ref<SystemSnapshot>(),
  hardwareSnapshot = ref<SystemSnapshot>(),
  busy = ref(false),
  error = ref(''),
  auto = ref(false),
  hardwareLive = ref(true),
  search = ref(''),
  foreground = ref(!document.hidden);
const active = computed(() => props.visible && props.session.connected && foreground.value);
let timer: ReturnType<typeof setTimeout> | undefined,
  generation = 0;
const detail = ref<{
  kind: 'service' | 'process';
  name: string;
  pid?: number;
  startTicks?: number;
  tab: 'status' | 'file';
  output: string;
  busy: boolean;
  error: string;
}>();
const pending = ref<{
  request: SystemRequest;
  title: string;
  description: string;
  elevated: boolean;
  automatic: boolean;
}>();
let detailGeneration = 0;
const processes = computed(
  () =>
    snapshot.value?.processes.filter((p) =>
      `${p.pid} ${p.name} ${p.user}`.toLowerCase().includes(search.value.toLowerCase()),
    ) || [],
);
const services = computed(
  () =>
    snapshot.value?.services.filter((s) =>
      `${s.name} ${s.description} ${s.fileState}`
        .toLowerCase()
        .includes(search.value.toLowerCase()),
    ) || [],
);
function bytes(n: number) {
  return n >= 1073741824 ? `${(n / 1073741824).toFixed(1)} GiB` : `${(n / 1048576).toFixed(1)} MiB`;
}
function fileState(state: string) {
  return (
    (
      {
        get enabled() {
          return tr('Aktiviert');
        },
        get 'enabled-runtime'() {
          return tr('Aktiviert · temporär');
        },
        get disabled() {
          return tr('Deaktiviert');
        },
        get static() {
          return tr('Statisch');
        },
        get masked() {
          return tr('Maskiert');
        },
        get indirect() {
          return tr('Indirekt');
        },
        get generated() {
          return tr('Generiert');
        },
        alias: 'Alias',
        get unknown() {
          return tr('Unbekannt');
        },
      } as Record<string, string>
    )[state] || state
  );
}
function preview(): SystemSnapshot {
  const elapsed = Date.now() / 1000;
  return {
    section: section.value,
    capturedAt: Math.floor(elapsed),
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
        get description() {
          return tr('PostgreSQL database server');
        },
        fileState: 'enabled',
      },
      {
        name: 'nginx.service',
        load: 'loaded',
        active: 'active',
        state: 'running',
        description: 'A high performance web server',
        fileState: 'enabled',
      },
      {
        name: 'backup.service',
        load: 'loaded',
        active: 'inactive',
        state: 'dead',
        get description() {
          return tr('Nightly backup');
        },
        fileState: 'disabled',
      },
    ],
    hardware: {
      hostname: props.session.name,
      get os() {
        return tr('Debian GNU/Linux 13');
      },
      get kernel() {
        return tr('Linux 6.12');
      },
      architecture: 'x86_64',
      get cpuModel() {
        return tr('AMD EPYC');
      },
      logicalCpus: 4,
      memoryTotal: 8589934592,
      memoryAvailable: 6442450944 + Math.sin(elapsed / 8) * 209715200,
      swapTotal: 2147483648,
      swapFree: 2147483648,
      uptime: 1067220 + elapsed - previewStarted,
      load: [0.14, 0.22, 0.19],
      cpuTicks: { total: elapsed * 400, idle: elapsed * 310 + Math.sin(elapsed / 10) * 100 },
      io: [{ name: 'vda', read: elapsed * 102400, written: elapsed * 61440 }],
      network: [{ name: 'eth0', received: elapsed * 204800, sent: elapsed * 92160 }],
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
  if (active.value && (section.value === 'hardware' ? hardwareLive.value : auto.value))
    timer = setTimeout(() => void refresh(), 5000);
}
async function refresh() {
  clearTimeout(timer);
  if (!active.value || busy.value) return;
  busy.value = true;
  error.value = '';
  const request = ++generation,
    wanted = section.value;
  try {
    const result = props.session.preview
      ? preview()
      : await invoke<SystemSnapshot>('ssh_system_snapshot', {
          id: props.session.id,
          section: wanted,
        });
    if (request === generation && wanted === section.value) {
      snapshot.value = result;
      if (wanted === 'hardware') hardwareSnapshot.value = result;
    }
  } catch (e) {
    if (request === generation) error.value = String(e);
  } finally {
    busy.value = false;
    if (active.value && wanted !== section.value) void refresh();
    else schedule();
  }
}
function visibility() {
  foreground.value = !document.hidden;
}
function openService(service: SystemSnapshot['services'][number]) {
  detail.value = {
    kind: 'service',
    name: service.name,
    tab: 'status',
    output: '',
    busy: false,
    error: '',
  };
  void loadDetail();
}
function openProcess(process: SystemSnapshot['processes'][number]) {
  detail.value = {
    kind: 'process',
    name: process.name,
    pid: process.pid,
    tab: 'status',
    output: '',
    busy: false,
    error: '',
  };
  void loadDetail();
}
function detailRequest(): SystemRequest | undefined {
  const d = detail.value;
  if (!d) return;
  return d.kind === 'process'
    ? { kind: 'processInfo', pid: d.pid! }
    : { kind: d.tab === 'file' ? 'serviceFile' : 'serviceStatus', name: d.name };
}
async function loadDetail() {
  const d = detail.value,
    request = detailRequest();
  if (!d || !request) return;
  const token = ++detailGeneration;
  d.busy = true;
  d.output = '';
  d.error = '';
  try {
    const result: SystemReply = props.session.preview
      ? {
          type: 'completed',
          startTicks: d.kind === 'process' ? 123456 : null,
          storage: null,
          output:
            d.kind === 'process'
              ? tr(
                  'Name:\t{p0}\nPID:\t{p1}\nExecutable: /usr/bin/{p2}\nArbeitsverzeichnis: /\nState:\tS (sleeping)\nThreads:\t2\nVmRSS:\t32768 kB',
                  { p0: d.name, p1: d.pid, p2: d.name },
                )
              : d.tab === 'file'
                ? `# /etc/systemd/system/${d.name}\n[Unit]\nDescription=${d.name}\nAfter=network.target\n\n[Service]\nType=simple\nExecStart=/usr/sbin/${d.name.split('.')[0]}\nRestart=on-failure\n\n[Install]\nWantedBy=multi-user.target`
                : tr(
                    'Id={p0}\nLoadState=loaded\nActiveState=active\nSubState=running\nUnitFileState=enabled\nMainPID=1432\nFragmentPath=/etc/systemd/system/{p1}\n\n--- STATUS & LETZTE LOGS ---\n● {p2}\n   Loaded: loaded; enabled\n   Active: active (running)\n\nBrowser-Vorschau mit Beispieldaten.',
                    { p0: d.name, p1: d.name, p2: d.name },
                  ),
        }
      : await invoke('ssh_system_control', {
          id: props.session.id,
          request,
          elevated: false,
          password: null,
        });
    if (token === detailGeneration && detail.value === d && result.type === 'completed') {
      d.output = result.output;
      if (result.startTicks) d.startTicks = result.startTicks;
    }
  } catch (e) {
    if (detail.value === d) d.error = String(e);
  } finally {
    d.busy = false;
  }
}
function action(
  request: SystemRequest,
  title: string,
  description: string,
  elevated = true,
  automatic = false,
) {
  pending.value = { request, title, description, elevated, automatic };
}
function serviceAction(
  kind:
    | 'serviceStart'
    | 'serviceStop'
    | 'serviceRestart'
    | 'serviceReload'
    | 'serviceEnable'
    | 'serviceDisable',
  label: string,
) {
  const d = detail.value;
  if (!d) return;
  action(
    { kind, name: d.name },
    `${label}: ${d.name}`,
    kind === 'serviceEnable' || kind === 'serviceDisable'
      ? tr(
          'Ändert den Autostart dieses Dienstes. Der aktuelle Laufzustand wird dabei nicht verändert.',
        )
      : tr(
          'Führt „{p0}“ für diesen Dienst aus. Laufende Verbindungen oder andere Dienste können betroffen sein.',
          { p0: label },
        ),
  );
}
function processAction(force = false) {
  const d = detail.value;
  if (!d?.pid || !d.startTicks) return;
  action(
    { kind: force ? 'processKill' : 'processTerminate', pid: d.pid, startTicks: d.startTicks },
    `${force ? tr('Sofort beenden') : tr('Beenden')}: ${d.name}`,
    tr('PID {p0} erhält {p1}', {
      p0: d.pid,
      p1: force
        ? tr('SIGKILL; der Prozess kann keine Daten mehr sichern.')
        : tr('SIGTERM und kann sich geordnet beenden.'),
    }),
    false,
  );
}
async function completed(result: Extract<SystemReply, { type: 'completed' }>) {
  const request = pending.value?.request;
  pending.value = undefined;
  if (
    ['serviceStatus', 'serviceFile', 'processInfo'].includes(request?.kind || '') &&
    detail.value
  ) {
    detail.value.output = result.output;
    if (result.startTicks) detail.value.startTicks = result.startTicks;
    return;
  }
  if (request?.kind === 'processTerminate' || request?.kind === 'processKill') closeDetail();
  notify(result.output.trim() || tr('Aktion ausgeführt'));
  if (detail.value?.kind === 'service') void loadDetail();
  if (active.value) void refresh();
}
function rootDetail() {
  const request = detailRequest();
  if (request)
    action(
      request,
      tr('Details mit sudo laden'),
      tr('Liest Status, Unit-Datei bzw. Prozessdetails mit erhöhten Rechten.'),
      true,
      true,
    );
}
function closeDetail() {
  ++detailGeneration;
  detail.value = undefined;
}
watch(
  [active, section],
  () => {
    clearTimeout(timer);
    if (active.value) void refresh();
  },
  { immediate: true },
);
watch([auto, hardwareLive], schedule);
onMounted(() => document.addEventListener('visibilitychange', visibility));
onBeforeUnmount(() => {
  ++generation;
  ++detailGeneration;
  clearTimeout(timer);
  document.removeEventListener('visibilitychange', visibility);
});
</script>
<template>
  <div class="ssh-tools">
    <div class="tools-heading">
      <div>
        <span class="eyebrow">{{ tr('SSH ·') }} {{ session.host }}</span>
        <h2>{{ tr('Systemübersicht') }}</h2>
        <p>{{ tr('Prozesse, Dienste und Hardware über deine bestehende Verbindung.') }}</p>
      </div>
      <button class="button secondary small" :disabled="busy || !active" @click="refresh">
        <Icon name="RefreshCw" :size="14" :class="{ spin: busy }" />{{ tr('Aktualisieren') }}
      </button>
    </div>
    <p v-if="!session.connected" class="tools-notice">{{ tr('Sitzung beendet.') }}</p>
    <p v-if="error" class="error-message" role="alert">{{ errorText(error) }}</p>
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
          tab === 'processes' ? tr('Prozesse') : tab === 'services' ? tr('Dienste') : tr('Hardware')
        }}</button
      ><label v-if="section === 'hardware'" class="checkbox-row"
        ><input v-model="hardwareLive" type="checkbox" />{{ tr('Live · 5 s') }}</label
      ><label v-else class="checkbox-row"
        ><input v-model="auto" type="checkbox" />{{ tr('Automatisch · 5 s') }}</label
      >
    </div>
    <div class="system-filter">
      <div v-if="section !== 'hardware'" class="search-field">
        <Icon name="Search" :size="14" /><input
          v-model="search"
          :aria-label="tr('Prozesse oder Dienste filtern')"
          :placeholder="tr('In der Ansicht suchen …')"
        />
      </div>
      <span v-if="snapshot?.section === section"
        >{{ tr('Stand') }}
        {{ new Date(snapshot.capturedAt * 1000).toLocaleTimeString(formatLocale()) }}</span
      >
    </div>
    <template v-if="snapshot?.section === section"
      ><p v-if="snapshot.message" class="tools-notice">{{ snapshot.message }}</p>
      <div v-if="section === 'processes'" class="tools-table-wrap">
        <table class="tools-table">
          <thead>
            <tr>
              <th>{{ tr('Prozess') }}</th>
              <th>{{ tr('PID') }}</th>
              <th>{{ tr('Benutzer') }}</th>
              <th :title="tr('Mittlere CPU-Nutzung seit Prozessstart')">{{ tr('CPU Ø') }}</th>
              <th>{{ tr('RAM') }}</th>
              <th></th>
            </tr>
          </thead>
          <tbody>
            <tr v-for="p in processes" :key="p.pid">
              <td>
                <button class="table-target mono" @click="openProcess(p)">{{ p.name }}</button>
              </td>
              <td class="mono">{{ p.pid }}</td>
              <td>{{ p.user }}</td>
              <td>{{ p.cpu.toFixed(1) }} %</td>
              <td>{{ bytes(p.memory) }}</td>
              <td>
                <button
                  class="icon-button"
                  :aria-label="tr('Details zu Prozess {p0}', { p0: p.pid })"
                  @click="openProcess(p)"
                >
                  <Icon name="MoreHorizontal" :size="16" />
                </button>
              </td>
            </tr>
          </tbody>
        </table>
        <p class="tools-footnote">
          {{
            tr(
              'Bis zu 100 Prozesse nach mittlerer CPU-Nutzung seit Prozessstart. Klicke einen Prozess für Details und Signale.',
            )
          }}
        </p>
      </div>
      <div v-if="section === 'services' && !snapshot.message" class="tools-table-wrap">
        <table class="tools-table">
          <thead>
            <tr>
              <th>{{ tr('Dienst') }}</th>
              <th>{{ tr('Status') }}</th>
              <th>{{ tr('Autostart') }}</th>
              <th>{{ tr('Beschreibung') }}</th>
              <th></th>
            </tr>
          </thead>
          <tbody>
            <tr v-for="s in services" :key="s.name">
              <td>
                <button class="table-target mono" @click="openService(s)">{{ s.name }}</button>
              </td>
              <td>
                <span
                  class="service-state"
                  :class="{ running: s.active === 'active', failed: s.active === 'failed' }"
                  >{{ s.state }}</span
                >
              </td>
              <td>{{ fileState(s.fileState) }}</td>
              <td>{{ s.description }}</td>
              <td>
                <button
                  class="icon-button"
                  :aria-label="tr('Details zu {p0}', { p0: s.name })"
                  @click="openService(s)"
                >
                  <Icon name="MoreHorizontal" :size="16" />
                </button>
              </td>
            </tr>
          </tbody>
        </table>
        <p class="tools-footnote">
          {{
            tr(
              'Bis zu 400 geladene oder installierte systemd-Dienste. „Statisch“ bedeutet: kein eigenständiger Autostart-Schalter; solche Units können durch andere Units gestartet werden.',
            )
          }}
        </p>
      </div>
    </template>
    <HardwareView
      v-if="hardwareSnapshot?.hardware"
      v-show="section === 'hardware'"
      :session="session"
      :hardware="hardwareSnapshot.hardware"
      :captured-at="hardwareSnapshot.capturedAt"
      :visible="visible && section === 'hardware'"
    />
  </div>
  <Modal
    v-if="detail"
    :title="detail.kind === 'service' ? detail.name : `${detail.name} · PID ${detail.pid}`"
    :subtitle="session.host"
    wide
    @close="closeDetail"
    ><div class="form-body">
      <div class="detail-tools">
        <template v-if="detail.kind === 'service'"
          ><button
            class="button secondary small"
            :class="{ selected: detail.tab === 'status' }"
            :disabled="detail.busy"
            @click="
              detail.tab = 'status';
              loadDetail();
            "
          >
            {{ tr('Status & Logs') }}</button
          ><button
            class="button secondary small"
            :class="{ selected: detail.tab === 'file' }"
            :disabled="detail.busy"
            @click="
              detail.tab = 'file';
              loadDetail();
            "
          >
            {{ tr('Unit-Dateien') }}
          </button></template
        ><button
          class="button secondary small"
          :disabled="detail.busy || !session.connected"
          @click="loadDetail"
        >
          <Icon name="RefreshCw" :size="14" :class="{ spin: detail.busy }" />{{
            tr('Aktualisieren')
          }}</button
        ><button
          class="button secondary small"
          :disabled="detail.busy || !session.connected"
          @click="rootDetail"
        >
          <Icon name="ShieldCheck" :size="14" />{{ tr('Mit sudo lesen') }}
        </button>
      </div>
      <p v-if="detail.error" class="error-message">{{ errorText(detail.error) }}</p>
      <pre class="system-output">{{ detail.busy ? tr('Wird geladen …') : detail.output }}</pre>
      <div v-if="detail.kind === 'service'" class="detail-tools">
        <button
          class="button secondary small"
          :disabled="!session.connected"
          @click="serviceAction('serviceStart', tr('Starten'))"
        >
          <Icon name="Play" :size="14" />{{ tr('Starten') }}</button
        ><button
          class="button secondary small"
          :disabled="!session.connected"
          @click="serviceAction('serviceStop', tr('Stoppen'))"
        >
          <Icon name="Square" :size="14" />{{ tr('Stoppen') }}</button
        ><button
          class="button secondary small"
          :disabled="!session.connected"
          @click="serviceAction('serviceRestart', tr('Neustarten'))"
        >
          <Icon name="RefreshCw" :size="14" />{{ tr('Restart') }}</button
        ><button
          class="button secondary small"
          :disabled="!session.connected"
          @click="serviceAction('serviceReload', tr('Neu laden'))"
        >
          {{ tr('Reload') }}</button
        ><button
          class="button secondary small"
          :disabled="!session.connected"
          @click="serviceAction('serviceEnable', tr('Autostart aktivieren'))"
        >
          {{ tr('Autostart an') }}</button
        ><button
          class="button secondary small"
          :disabled="!session.connected"
          @click="serviceAction('serviceDisable', tr('Autostart deaktivieren'))"
        >
          {{ tr('Autostart aus') }}
        </button>
      </div>
      <div v-else class="detail-tools">
        <button
          class="button secondary small"
          :disabled="!detail.startTicks || !session.connected"
          @click="processAction(false)"
        >
          {{ tr('Beenden · SIGTERM') }}</button
        ><button
          class="button danger small"
          :disabled="!detail.startTicks || !session.connected"
          @click="processAction(true)"
        >
          {{ tr('Sofort beenden · SIGKILL') }}
        </button>
      </div>
    </div></Modal
  >
  <RemoteActionDialog
    v-if="pending"
    :session="session"
    :request="pending.request"
    :title="pending.title"
    :description="pending.description"
    :elevated="pending.elevated"
    :automatic="pending.automatic"
    @close="pending = undefined"
    @completed="completed"
  />
</template>
