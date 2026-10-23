<script setup lang="ts">
import { computed, ref, watch } from 'vue';
import { invoke } from '@tauri-apps/api/core';
import type { ClientSession, HardwareInfo, StorageInfo, SystemReply } from '../types';
import SelectMenu from './SelectMenu.vue';
import MetricGraph from './MetricGraph.vue';
import RemoteActionDialog from './RemoteActionDialog.vue';
import Icon from './Icon.vue';
const props = defineProps<{
  session: ClientSession;
  hardware: HardwareInfo;
  capturedAt: number;
  visible: boolean;
}>();
const history = ref<
  {
    time: number;
    cpu: number | null;
    ram: number;
    load: number;
    io: HardwareInfo['io'];
    network: HardwareInfo['network'];
    uptime: number;
  }[]
>([]);
let previous: { time: number; hardware: HardwareInfo } | undefined;
const disk = ref(''),
  network = ref(''),
  storage = ref<StorageInfo>(),
  storageBusy = ref(false),
  storageError = ref(''),
  rootStorage = ref(false);
function bytes(n: number) {
  return n >= 1073741824
    ? `${(n / 1073741824).toFixed(1)} GiB`
    : n >= 1048576
      ? `${(n / 1048576).toFixed(1)} MiB`
      : `${(n / 1024).toFixed(1)} KiB`;
}
function lvmBytes(value: string | undefined) {
  const n = Number((value || '').trim());
  return Number.isFinite(n) ? bytes(n) : value || '—';
}
watch(
  () => props.capturedAt,
  (time) => {
    if (previous?.time === time) return;
    const hw = props.hardware;
    if (previous && (time <= previous.time || hw.uptime < previous.hardware.uptime)) {
      history.value = [];
      previous = undefined;
    }
    const total = previous ? hw.cpuTicks.total - previous.hardware.cpuTicks.total : 0;
    const idle = previous ? hw.cpuTicks.idle - previous.hardware.cpuTicks.idle : 0;
    const cpu =
      total > 0 && idle >= 0 ? Math.max(0, Math.min(100, (1 - idle / total) * 100)) : null;
    history.value = [
      ...history.value,
      {
        time,
        cpu,
        ram: hw.memoryTotal ? ((hw.memoryTotal - hw.memoryAvailable) / hw.memoryTotal) * 100 : 0,
        load: hw.load[0] || 0,
        io: hw.io,
        network: hw.network,
        uptime: hw.uptime,
      },
    ].slice(-120);
    previous = { time, hardware: hw };
    if (!hw.io.some((d) => d.name === disk.value))
      disk.value = hw.io.find((d) => !/^(dm-|md)/.test(d.name))?.name || hw.io[0]?.name || '';
    if (!hw.network.some((n) => n.name === network.value))
      network.value = hw.network.find((n) => n.name !== 'lo')?.name || hw.network[0]?.name || '';
  },
  { immediate: true },
);
const cpuPoints = computed(() => history.value.map((p) => ({ time: p.time, values: [p.cpu] })));
const ramPoints = computed(() => history.value.map((p) => ({ time: p.time, values: [p.ram] })));
const loadPoints = computed(() => history.value.map((p) => ({ time: p.time, values: [p.load] })));
const ioPoints = computed(() =>
  history.value.map((p, i) => {
    const prev = history.value[i - 1],
      current = p.io.find((d) => d.name === disk.value),
      old = prev?.io.find((d) => d.name === disk.value),
      dt = prev ? p.uptime - prev.uptime : 0;
    return {
      time: p.time,
      values:
        current && old && dt > 0 && current.read >= old.read && current.written >= old.written
          ? [(current.read - old.read) / dt / 1024, (current.written - old.written) / dt / 1024]
          : [null, null],
    };
  }),
);
const netPoints = computed(() =>
  history.value.map((p, i) => {
    const prev = history.value[i - 1],
      current = p.network.find((n) => n.name === network.value),
      old = prev?.network.find((n) => n.name === network.value),
      dt = prev ? p.uptime - prev.uptime : 0;
    return {
      time: p.time,
      values:
        current && old && dt > 0 && current.received >= old.received && current.sent >= old.sent
          ? [(current.received - old.received) / dt / 1024, (current.sent - old.sent) / dt / 1024]
          : [null, null],
    };
  }),
);
function previewStorage(): StorageInfo {
  return {
    mounts: [
      {
        source: '/dev/mapper/system-root',
        target: '/',
        filesystem: 'ext4',
        options: 'rw,relatime',
      },
      { source: '/dev/vda1', target: '/boot', filesystem: 'ext4', options: 'rw,relatime' },
    ],
    pvs: [
      { pv_name: '/dev/vda2', pv_size: '107374182400', pv_free: '21474836480', vg_name: 'system' },
    ],
    vgs: [
      {
        vg_name: 'system',
        vg_size: '107374182400',
        vg_free: '21474836480',
        pv_count: '1',
        lv_count: '2',
      },
    ],
    lvs: [
      {
        lv_name: 'root',
        vg_name: 'system',
        lv_size: '75161927680',
        lv_attr: '-wi-ao----',
        lv_path: '/dev/system/root',
        pool_lv: '',
        origin: '',
      },
      {
        lv_name: 'swap',
        vg_name: 'system',
        lv_size: '10737418240',
        lv_attr: '-wi-ao----',
        lv_path: '/dev/system/swap',
        pool_lv: '',
        origin: '',
      },
    ],
    messages: [],
  };
}
async function loadStorage() {
  if (storageBusy.value) return;
  storageBusy.value = true;
  storageError.value = '';
  try {
    if (props.session.preview) storage.value = previewStorage();
    else {
      const reply = await invoke<SystemReply>('ssh_system_control', {
        id: props.session.id,
        request: { kind: 'storage' },
        elevated: false,
        password: null,
      });
      if (reply.type === 'completed' && reply.storage) storage.value = reply.storage;
    }
  } catch (e) {
    storageError.value = String(e);
  } finally {
    storageBusy.value = false;
  }
}
function rootReply(reply: Extract<SystemReply, { type: 'completed' }>) {
  if (reply.storage) storage.value = reply.storage;
  rootStorage.value = false;
}
</script>
<template>
  <div class="hardware-host">
    <div class="hardware-view">
      <div class="hardware-cards">
        <article>
          <Icon name="Server" :size="20" /><small>System</small
          ><strong>{{ hardware.hostname }}</strong>
          <p>{{ hardware.os }}<br />{{ hardware.kernel }} · {{ hardware.architecture }}</p>
        </article>
        <article>
          <Icon name="Cpu" :size="20" /><small>Prozessor</small
          ><strong>{{ hardware.logicalCpus }} logische CPUs</strong>
          <p>
            {{ hardware.cpuModel }}<br />Laufzeit: {{ Math.floor(hardware.uptime / 86400) }} Tage,
            {{ Math.floor((hardware.uptime % 86400) / 3600) }} Stunden
          </p>
        </article>
        <article>
          <Icon name="MemoryStick" :size="20" /><small>Arbeitsspeicher</small
          ><strong
            >{{ bytes(hardware.memoryTotal - hardware.memoryAvailable) }} /
            {{ bytes(hardware.memoryTotal) }}</strong
          >
          <p>
            {{ bytes(hardware.memoryAvailable) }} verfügbar<br />Swap:
            {{ bytes(hardware.swapTotal - hardware.swapFree) }} / {{ bytes(hardware.swapTotal) }}
          </p>
        </article>
      </div>
      <div class="metrics-grid">
        <MetricGraph
          title="CPU-Auslastung"
          unit="%"
          :points="cpuPoints"
          :labels="['CPU']"
          :limit="100"
        /><MetricGraph
          title="Arbeitsspeicher"
          unit="%"
          :points="ramPoints"
          :labels="['Belegt']"
          :limit="100"
          :colors="['var(--chart-3)']"
        /><MetricGraph
          title="Systemlast · 1 Minute"
          unit=""
          :points="loadPoints"
          :labels="['Load']"
          :colors="['var(--chart-4)']"
        />
        <div v-if="hardware.io.length" class="metric-with-picker">
          <SelectMenu
            v-model="disk"
            label="Datenträger für I/O-Verlauf"
            :options="hardware.io.map((d) => ({ value: d.name, label: d.name }))"
          /><MetricGraph
            title="Datenträger-I/O"
            unit="KiB/s"
            :points="ioPoints"
            :labels="['Lesen', 'Schreiben']"
          />
        </div>
        <div v-if="hardware.network.length" class="metric-with-picker">
          <SelectMenu
            v-model="network"
            label="Netzwerkschnittstelle für Verlauf"
            :options="hardware.network.map((n) => ({ value: n.name, label: n.name }))"
          /><MetricGraph
            title="Netzwerk"
            unit="KiB/s"
            :points="netPoints"
            :labels="['Empfangen', 'Senden']"
          />
        </div>
      </div>
      <p class="tools-footnote">
        Bis zu 120 Messungen. CPU und I/O werden aus Zählerdifferenzen berechnet; nach der ersten
        Messung ist ein weiteres Sample nötig. Live-Abfragen pausieren, sobald die Ansicht verborgen
        ist.
      </p>
      <h3 class="storage-heading">Dateisysteme & Belegung</h3>
      <table class="tools-table">
        <thead>
          <tr>
            <th>Gerät</th>
            <th>Mount</th>
            <th>Belegt</th>
            <th>Verfügbar</th>
          </tr>
        </thead>
        <tbody>
          <tr v-for="d in hardware.disks" :key="d.name + d.mount">
            <td class="mono">{{ d.name }}</td>
            <td class="mono">{{ d.mount }}</td>
            <td>{{ bytes(d.used) }} / {{ bytes(d.total) }}</td>
            <td>{{ bytes(d.available) }}</td>
          </tr>
        </tbody>
      </table>
      <div class="storage-toolbar">
        <div>
          <h3>Mounts & LVM</h3>
          <p class="muted">
            Gezielt laden · LVM wird nur abgefragt, wenn die Werkzeuge vorhanden sind.
          </p>
        </div>
        <button
          class="button secondary small"
          :disabled="storageBusy || !session.connected || !visible"
          @click="loadStorage"
        >
          <Icon name="RefreshCw" :size="14" :class="{ spin: storageBusy }" />{{
            storage ? 'Aktualisieren' : 'Laden'
          }}</button
        ><button
          class="button secondary small"
          :disabled="storageBusy || !session.connected || !visible"
          @click="rootStorage = true"
        >
          <Icon name="ShieldCheck" :size="14" />Mit sudo laden
        </button>
      </div>
      <p v-if="storageError" class="error-message">{{ storageError }}</p>
      <template v-if="storage"
        ><p v-for="message in storage.messages" :key="message" class="tools-notice">
          {{ message }}
        </p>
        <h3 class="storage-heading">
          Mounts <span class="muted">{{ storage.mounts.length }}</span>
        </h3>
        <div class="tools-table-wrap">
          <table class="tools-table">
            <thead>
              <tr>
                <th>Ziel</th>
                <th>Quelle</th>
                <th>Dateisystem</th>
                <th>Optionen</th>
              </tr>
            </thead>
            <tbody>
              <tr v-for="m in storage.mounts" :key="m.target + m.source">
                <td class="mono">{{ m.target }}</td>
                <td class="mono">{{ m.source }}</td>
                <td>{{ m.filesystem }}</td>
                <td class="mount-options">{{ m.options }}</td>
              </tr>
            </tbody>
          </table>
        </div>
        <h3 class="storage-heading">Physical Volumes · PV</h3>
        <table v-if="storage.pvs.length" class="tools-table">
          <thead>
            <tr>
              <th>Gerät</th>
              <th>Volume Group</th>
              <th>Größe</th>
              <th>Frei</th>
            </tr>
          </thead>
          <tbody>
            <tr v-for="pv in storage.pvs" :key="pv.pv_name">
              <td class="mono">{{ pv.pv_name }}</td>
              <td>{{ pv.vg_name }}</td>
              <td>{{ lvmBytes(pv.pv_size) }}</td>
              <td>{{ lvmBytes(pv.pv_free) }}</td>
            </tr>
          </tbody>
        </table>
        <p v-else class="muted">Keine lesbaren Physical Volumes.</p>
        <h3 class="storage-heading">Volume Groups · VG</h3>
        <table v-if="storage.vgs.length" class="tools-table">
          <thead>
            <tr>
              <th>Name</th>
              <th>Größe</th>
              <th>Frei</th>
              <th>PVs / LVs</th>
            </tr>
          </thead>
          <tbody>
            <tr v-for="vg in storage.vgs" :key="vg.vg_name">
              <td class="mono">{{ vg.vg_name }}</td>
              <td>{{ lvmBytes(vg.vg_size) }}</td>
              <td>{{ lvmBytes(vg.vg_free) }}</td>
              <td>{{ vg.pv_count }} / {{ vg.lv_count }}</td>
            </tr>
          </tbody>
        </table>
        <p v-else class="muted">Keine lesbaren Volume Groups.</p>
        <h3 class="storage-heading">Logical Volumes · LV</h3>
        <table v-if="storage.lvs.length" class="tools-table">
          <thead>
            <tr>
              <th>Pfad</th>
              <th>VG / LV</th>
              <th>Größe</th>
              <th>Attribute</th>
              <th>Pool / Origin</th>
            </tr>
          </thead>
          <tbody>
            <tr v-for="lv in storage.lvs" :key="lv.vg_name + '/' + lv.lv_name">
              <td class="mono">{{ lv.lv_path }}</td>
              <td>{{ lv.vg_name }} / {{ lv.lv_name }}</td>
              <td>{{ lvmBytes(lv.lv_size) }}</td>
              <td class="mono">{{ lv.lv_attr }}</td>
              <td>{{ lv.pool_lv || '—' }} / {{ lv.origin || '—' }}</td>
            </tr>
          </tbody>
        </table>
        <p v-else class="muted">Keine lesbaren Logical Volumes.</p>
      </template>
    </div>
    <RemoteActionDialog
      v-if="rootStorage"
      :session="session"
      :request="{ kind: 'storage' }"
      title="Speicherstruktur mit sudo laden"
      description="Mounts und vorhandene LVM-Berichte nur lesen. Es werden keine Datenträger verändert."
      :elevated="true"
      automatic
      @close="rootStorage = false"
      @completed="rootReply"
    />
  </div>
</template>
