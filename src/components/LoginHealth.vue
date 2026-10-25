<script setup lang="ts">
import { computed, ref } from 'vue';
import type { ClientSession } from '../types';
import { loadLoginHealth } from '../composables/useNoryn';
import Icon from './Icon.vue';
const props = defineProps<{ session: ClientSession }>();
const allInactive = ref(false);
const report = computed(() => props.session.health);
const services = computed(
  () =>
    report.value?.services.filter(
      (s) => allInactive.value || s.state === 'failed' || s.becameInactive,
    ) || [],
);
function date(at: number) {
  return new Date(at * 1000).toLocaleString();
}
function uptime(seconds: number) {
  const days = Math.floor(seconds / 86400);
  return `${days ? `${days} T · ` : ''}${Math.floor((seconds % 86400) / 3600)} Std · ${Math.floor((seconds % 3600) / 60)} Min`;
}
</script>
<template>
  <section class="login-health">
    <div class="tools-heading">
      <div>
        <span class="eyebrow">LOGIN-CHECK · {{ session.host }}</span>
        <h2>Dein Server auf einen Blick.</h2>
        <p>Einmalig beim Verbinden gelesen · kein Monitoring im Hintergrund.</p>
      </div>
      <span v-if="session.healthLoading" class="muted"
        ><Icon name="LoaderCircle" class="spin" :size="16" />Daten werden gelesen …</span
      >
    </div>
    <p v-if="session.healthError" class="error-message" role="alert">{{ session.healthError }}</p>
    <button
      v-if="session.healthError"
      class="button secondary"
      :disabled="session.healthLoading || !session.connected"
      @click="loadLoginHealth(session)"
    >
      Erneut abfragen
    </button>
    <template v-if="report">
      <p class="muted">
        Stand: {{ date(report.capturedAt) }} ·
        {{
          report.previousLogin
            ? `Letzte Anmeldung auf diesem Gerät: ${date(report.previousLogin)}`
            : 'Erste bekannte Anmeldung mit diesem Eintrag und Zugang.'
        }}
      </p>
      <div class="health-cards">
        <article>
          <Icon name="Clock" :size="20" /><span>Laufzeit</span
          ><strong>{{ uptime(report.uptime) }}</strong
          ><small>{{ report.cores }} logische CPUs</small>
        </article>
        <article>
          <Icon name="Activity" :size="20" /><span>Dienste aktuell</span
          ><strong>{{
            report.servicesAvailable
              ? `${report.services.filter((s) => s.state === 'failed').length} fehlgeschlagen`
              : 'Nicht verfügbar'
          }}</strong
          ><small
            >{{ report.services.filter((s) => s.becameInactive).length }} zuvor aktive Dienste jetzt
            inaktiv/fehlgeschlagen</small
          >
        </article>
        <article>
          <Icon name="Download" :size="20" /><span>Updates im Paketcache</span
          ><strong>{{ report.updates ? report.updates.total : 'Unbekannt' }}</strong
          ><small>{{
            report.updates
              ? report.updates.security === null
                ? 'Sicherheitsrelevanz nicht verfügbar'
                : `${report.updates.security} mit Sicherheitskennzeichnung`
              : 'Paketmanager, Cache oder Zugriff nicht verfügbar'
          }}</small>
        </article>
        <article>
          <Icon name="Cpu" :size="20" /><span>Lastspitzen · Load 1 Min</span
          ><strong>{{
            report.loadSamples
              ? report.peaks.length
                ? `${report.peaks.length} aufgezeichnete Spitzen`
                : 'Keine im gelesenen Zeitraum'
              : 'Keine Historie verfügbar'
          }}</strong
          ><small>Load &gt; {{ report.cores }} · erste 5 Boot-Minuten ausgeschlossen</small>
        </article>
      </div>
      <article class="health-section">
        <div class="tools-heading">
          <div>
            <h3>Dienstzustände</h3>
            <p>Aktuell fehlgeschlagen oder gegenüber der letzten Anmeldung inaktiv.</p>
          </div>
          <label class="checkbox-row"
            ><input v-model="allInactive" type="checkbox" />Alle inaktiven Dienste zeigen</label
          >
        </div>
        <div v-for="service in services" :key="service.name" class="health-row">
          <strong class="mono">{{ service.name }}</strong
          ><span class="small-badge">{{
            service.state === 'failed' ? 'Fehlgeschlagen' : 'Inaktiv'
          }}</span
          ><span>{{
            service.becameInactive ? 'Seit dem letzten Vergleich inaktiv' : service.description
          }}</span>
        </div>
        <p v-if="!services.length" class="muted">
          {{
            report.servicesAvailable
              ? 'Keine entsprechenden Dienste in der gelesenen Liste.'
              : 'Dienststatus konnte nicht gelesen werden.'
          }}
        </p>
        <small class="muted"
          >Bis zu 400 geladene Dienste. Inaktiv kann bei einmaligen oder zeitgesteuerten Diensten
          normal sein.</small
        >
      </article>
      <article class="health-section">
        <h3>Prozess- und Dienstfehler seit der letzten Anmeldung</h3>
        <div
          v-for="event in report.events"
          :key="`${event.at}-${event.unit}-${event.message}`"
          class="health-event"
        >
          <time>{{ date(event.at) }}</time
          ><strong class="mono">{{ event.unit }}</strong>
          <p>{{ event.message }}</p>
        </div>
        <p v-if="!report.events.length" class="muted">
          {{
            report.journalAvailable
              ? 'Keine passenden Ereignisse im lesbaren Journal-Ausschnitt.'
              : 'Keine lesbare Diensthistorie verfügbar.'
          }}
        </p>
      </article>
      <article class="health-section">
        <h3>Wann war der Server ausgelastet?</h3>
        <p class="muted">
          {{
            report.loadSamples
              ? `${report.loadSamples} sysstat-Messpunkte gelesen; bis zu 100 Spitzen. Zeitraum ab ${date(report.loadWindow)} bis ${date(report.capturedAt)}.`
              : 'Für historische Lastwerte muss das Zielsystem bereits sysstat-Daten aufzeichnen. Noryn installiert und startet keinen Sammler.'
          }}
          Die Auswertung umfasst höchstens sieben Tage des aktuellen Boots.
        </p>
        <div v-for="peak in report.peaks" :key="peak.at" class="health-row">
          <time>{{ date(peak.at) }}</time
          ><strong class="mono">Load {{ peak.load.toFixed(2) }}</strong
          ><span>{{ (peak.load / report.cores).toFixed(1) }} × CPU-Anzahl</span>
        </div>
      </article>
      <details class="health-section">
        <summary>Datenquellen und Verfügbarkeit</summary>
        <p>
          Update-Zahlen stammen ausschließlich aus vorhandenen Paketlisten ({{
            report.updates?.manager || 'unbekannter Paketmanager'
          }}). Deren Aktualität hängt von der Paketpflege auf dem Server ab; es wird kein Repository
          aktualisiert. Dienste und Journale werden mit den Rechten des angemeldeten Benutzers
          gelesen.
        </p>
        <p v-for="note in report.notes" :key="note" class="muted">{{ note }}</p>
      </details>
    </template>
  </section>
</template>
