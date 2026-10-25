<script setup lang="ts">
import { messageText, errorText, t as tr, formatLocale } from '../i18n';

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
  return new Date(at * 1000).toLocaleString(formatLocale());
}
function uptime(seconds: number) {
  const days = Math.floor(seconds / 86400);
  return tr('{p0}{p1} Std · {p2} Min', {
    p0: days ? tr('{p0} T · ', { p0: days }) : '',
    p1: Math.floor((seconds % 86400) / 3600),
    p2: Math.floor((seconds % 3600) / 60),
  });
}
</script>
<template>
  <section class="login-health">
    <div class="tools-heading">
      <div>
        <span class="eyebrow">{{ tr('LOGIN-CHECK ·') }} {{ session.host }}</span>
        <h2>{{ tr('Dein Server auf einen Blick.') }}</h2>
        <p>{{ tr('Einmalig beim Verbinden gelesen · kein Monitoring im Hintergrund.') }}</p>
      </div>
      <span v-if="session.healthLoading" class="muted"
        ><Icon name="LoaderCircle" class="spin" :size="16" />{{
          tr('Daten werden gelesen …')
        }}</span
      >
    </div>
    <p v-if="session.healthError" class="error-message" role="alert">
      {{ errorText(session.healthError) }}
    </p>
    <button
      v-if="session.healthError"
      class="button secondary"
      :disabled="session.healthLoading || !session.connected"
      @click="loadLoginHealth(session)"
    >
      {{ tr('Erneut abfragen') }}
    </button>
    <template v-if="report">
      <p class="muted">
        {{ tr('Stand:') }} {{ date(report.capturedAt) }} ·
        {{
          report.previousLogin
            ? tr('Letzte Anmeldung auf diesem Gerät: {p0}', { p0: date(report.previousLogin) })
            : tr('Erste bekannte Anmeldung mit diesem Eintrag und Zugang.')
        }}
      </p>
      <div class="health-cards">
        <article>
          <Icon name="Clock" :size="20" /><span>{{ tr('Laufzeit') }}</span
          ><strong>{{ uptime(report.uptime) }}</strong
          ><small>{{ report.cores }} {{ tr('logische CPUs') }}</small>
        </article>
        <article>
          <Icon name="Activity" :size="20" /><span>{{ tr('Dienste aktuell') }}</span
          ><strong>{{
            report.servicesAvailable
              ? tr('{p0} fehlgeschlagen', {
                  p0: report.services.filter((s) => s.state === 'failed').length,
                })
              : tr('Nicht verfügbar')
          }}</strong
          ><small
            >{{ report.services.filter((s) => s.becameInactive).length }}
            {{ tr('zuvor aktive Dienste jetzt inaktiv/fehlgeschlagen') }}</small
          >
        </article>
        <article>
          <Icon name="Download" :size="20" /><span>{{ tr('Updates im Paketcache') }}</span
          ><strong>{{ report.updates ? report.updates.total : tr('Unbekannt') }}</strong
          ><small>{{
            report.updates
              ? report.updates.security === null
                ? tr('Sicherheitsrelevanz nicht verfügbar')
                : tr('{p0} mit Sicherheitskennzeichnung', { p0: report.updates.security })
              : tr('Paketmanager, Cache oder Zugriff nicht verfügbar')
          }}</small>
        </article>
        <article>
          <Icon name="Cpu" :size="20" /><span>{{ tr('Lastspitzen · Load 1 Min') }}</span
          ><strong>{{
            report.loadSamples
              ? report.peaks.length
                ? tr('{p0} aufgezeichnete Spitzen', { p0: report.peaks.length })
                : tr('Keine im gelesenen Zeitraum')
              : tr('Keine Historie verfügbar')
          }}</strong
          ><small
            >{{ tr('Load >') }} {{ report.cores }}
            {{ tr('· erste 5 Boot-Minuten ausgeschlossen') }}</small
          >
        </article>
      </div>
      <article class="health-section">
        <div class="tools-heading">
          <div>
            <h3>{{ tr('Dienstzustände') }}</h3>
            <p>{{ tr('Aktuell fehlgeschlagen oder gegenüber der letzten Anmeldung inaktiv.') }}</p>
          </div>
          <label class="checkbox-row"
            ><input v-model="allInactive" type="checkbox" />{{
              tr('Alle inaktiven Dienste zeigen')
            }}</label
          >
        </div>
        <div v-for="service in services" :key="service.name" class="health-row">
          <strong class="mono">{{ service.name }}</strong
          ><span class="small-badge">{{
            service.state === 'failed' ? tr('Fehlgeschlagen') : tr('Inaktiv')
          }}</span
          ><span>{{
            service.becameInactive ? tr('Seit dem letzten Vergleich inaktiv') : service.description
          }}</span>
        </div>
        <p v-if="!services.length" class="muted">
          {{
            report.servicesAvailable
              ? tr('Keine entsprechenden Dienste in der gelesenen Liste.')
              : tr('Dienststatus konnte nicht gelesen werden.')
          }}
        </p>
        <small class="muted">{{
          tr(
            'Bis zu 400 geladene Dienste. Inaktiv kann bei einmaligen oder zeitgesteuerten Diensten normal sein.',
          )
        }}</small>
      </article>
      <article class="health-section">
        <h3>{{ tr('Prozess- und Dienstfehler seit der letzten Anmeldung') }}</h3>
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
              ? tr('Keine passenden Ereignisse im lesbaren Journal-Ausschnitt.')
              : tr('Keine lesbare Diensthistorie verfügbar.')
          }}
        </p>
      </article>
      <article class="health-section">
        <h3>{{ tr('Wann war der Server ausgelastet?') }}</h3>
        <p class="muted">
          {{
            report.loadSamples
              ? tr(
                  '{p0} sysstat-Messpunkte gelesen; bis zu 100 Spitzen. Zeitraum ab {p1} bis {p2}.',
                  {
                    p0: report.loadSamples,
                    p1: date(report.loadWindow),
                    p2: date(report.capturedAt),
                  },
                )
              : tr(
                  'Für historische Lastwerte muss das Zielsystem bereits sysstat-Daten aufzeichnen. Noryn installiert und startet keinen Sammler.',
                )
          }}
          {{ tr('Die Auswertung umfasst höchstens sieben Tage des aktuellen Boots.') }}
        </p>
        <div v-for="peak in report.peaks" :key="peak.at" class="health-row">
          <time>{{ date(peak.at) }}</time
          ><strong class="mono">{{ tr('Load') }} {{ peak.load.toFixed(2) }}</strong
          ><span>{{ (peak.load / report.cores).toFixed(1) }} {{ tr('× CPU-Anzahl') }}</span>
        </div>
      </article>
      <details class="health-section">
        <summary>{{ tr('Datenquellen und Verfügbarkeit') }}</summary>
        <p>
          {{ tr('Update-Zahlen stammen ausschließlich aus vorhandenen Paketlisten (')
          }}{{ report.updates?.manager || tr('unbekannter Paketmanager')
          }}{{
            tr(
              '). Deren Aktualität hängt von der Paketpflege auf dem Server ab; es wird kein Repository aktualisiert. Dienste und Journale werden mit den Rechten des angemeldeten Benutzers gelesen.',
            )
          }}
        </p>
        <p v-for="note in report.notes" :key="note" class="muted">{{ messageText(note) }}</p>
      </details>
    </template>
  </section>
</template>
