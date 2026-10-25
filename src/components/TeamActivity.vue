<script setup lang="ts">
import { t as tr, formatLocale } from '../i18n';

import { computed, ref, watch } from 'vue';
import type { ChangedField, TeamChange, TeamInfo } from '../types';
import { desktop, store } from '../composables/useNoryn';
import Icon from './Icon.vue';
import Modal from './Modal.vue';
import SelectMenu from './SelectMenu.vue';

const props = defineProps<{ team: TeamInfo }>();
const emit = defineEmits<{ close: [] }>();
const query = ref('');
const actionFilter = ref('all');
const limit = ref(50);
const actions: Record<TeamChange['action'], string> = {
  get created() {
    return tr('Angelegt');
  },
  get updated() {
    return tr('Geändert');
  },
  get deleted() {
    return tr('Entfernt');
  },
  get shared() {
    return tr('Freigegeben');
  },
  get unshared() {
    return tr('Freigabe aufgehoben');
  },
  get imported() {
    return tr('Importiert');
  },
};
const objects: Record<TeamChange['object'], string> = {
  get command() {
    return tr('Befehl');
  },
  get folder() {
    return tr('Ordner');
  },
  get connection() {
    return tr('Verbindung');
  },
  get credential() {
    return tr('Zugangsdaten');
  },
  get workspace() {
    return tr('Tresor');
  },
};
const fields: Record<ChangedField, string> = {
  get command() {
    return tr('Befehlstext');
  },
  get name() {
    return tr('Name');
  },
  get parent() {
    return tr('Übergeordneter Ordner');
  },
  get kind() {
    return tr('Typ');
  },
  get host() {
    return tr('Host');
  },
  get port() {
    return tr('Port');
  },
  get protocol() {
    return tr('Protokoll');
  },
  get initialPath() {
    return tr('Startordner');
  },
  get notes() {
    return tr('Notizen');
  },
  get tags() {
    return tr('Kategorien');
  },
  get favorite() {
    return tr('Favorit');
  },
  get username() {
    return tr('Benutzername');
  },
  get privateKey() {
    return tr('Privater Schlüssel');
  },
  get passphrase() {
    return tr('Schlüssel-Passphrase');
  },
  get password() {
    return tr('Benutzerpasswort');
  },
  get children() {
    return tr('Enthaltene Einträge');
  },
};
const entries = computed(() =>
  (store.view.teamActivity[props.team.id] || [])
    .slice()
    .reverse()
    .sort((a, b) => b.occurredAt - a.occurredAt)
    .filter(
      (entry) =>
        (actionFilter.value === 'all' || entry.action === actionFilter.value) &&
        (!query.value.trim() ||
          [
            entry.actor,
            entry.targetName,
            objects[entry.object],
            ...entry.fields.map((field) => fields[field]),
          ]
            .join(' ')
            .toLocaleLowerCase(formatLocale())
            .includes(query.value.trim().toLocaleLowerCase(formatLocale()))),
    ),
);
const rows = computed(() => entries.value.slice(0, limit.value));
const date = computed(
  () => new Intl.DateTimeFormat(formatLocale(), { dateStyle: 'medium', timeStyle: 'medium' }),
);
watch([query, actionFilter], () => (limit.value = 50));
</script>
<template>
  <Modal :title="tr('Änderungsverlauf')" :subtitle="team.name" wide @close="emit('close')">
    <div class="form-body team-activity">
      <p v-if="!desktop" class="activity-preview">{{ tr('Browser-Vorschau · Beispieldaten') }}</p>
      <p>
        {{
          tr(
            'Wer hat was geändert? Die letzten 1.000 Änderungen werden mit dem Team verschlüsselt synchronisiert. Namen werden von den Mitgliedern selbst angegeben.',
          )
        }}
      </p>
      <p class="activity-sync-note">
        {{
          team.liveSync
            ? tr(
                'Bei aktivem Live-Sync erscheinen Änderungen automatisch. Konflikte werden unter Team-Sync angezeigt.',
              )
            : team.dirty
              ? tr('Enthält lokale Änderungen. Andere sehen sie nach deinem nächsten Upload.')
              : tr('Neue Änderungen anderer Mitglieder erscheinen nach dem Abrufen vom Server.')
        }}
      </p>
      <div class="activity-filters">
        <label class="search-field"
          ><Icon name="Search" :size="16" /><input
            v-model="query"
            :aria-label="tr('Änderungsverlauf durchsuchen')"
            :placeholder="tr('Name oder Eintrag suchen …')"
        /></label>
        <SelectMenu
          v-model="actionFilter"
          :label="tr('Änderungen filtern')"
          :options="[
            {
              value: 'all',
              get label() {
                return tr('Alle Änderungen');
              },
            },
            ...Object.entries(actions).map(([value, label]) => ({ value, label })),
          ]"
        />
      </div>
      <div v-if="rows.length" class="activity-table-wrap">
        <table class="activity-table">
          <thead>
            <tr>
              <th>{{ tr('Wann') }}</th>
              <th>{{ tr('Wer') }}</th>
              <th>{{ tr('Änderung') }}</th>
            </tr>
          </thead>
          <tbody>
            <tr v-for="entry in rows" :key="entry.id">
              <td>
                <time :datetime="new Date(entry.occurredAt).toISOString()">{{
                  date.format(entry.occurredAt)
                }}</time>
              </td>
              <td>{{ entry.actor }}</td>
              <td>
                <span class="activity-action" :class="entry.action">{{
                  actions[entry.action]
                }}</span
                ><strong>{{ entry.targetName }}</strong
                ><span
                  >{{ objects[entry.object]
                  }}{{
                    entry.fields.length
                      ? ' · ' + entry.fields.map((field) => fields[field]).join(', ')
                      : ''
                  }}</span
                >
              </td>
            </tr>
          </tbody>
        </table>
      </div>
      <p v-else class="activity-empty">
        {{
          query || actionFilter !== 'all'
            ? tr('Keine passenden Änderungen.')
            : tr(
                'Noch keine protokollierten Änderungen. Ältere Änderungen werden nicht nachträglich zugeordnet.',
              )
        }}
      </p>
      <button v-if="entries.length > limit" class="button secondary" @click="limit += 50">
        {{ tr('Weitere Änderungen anzeigen (') }}{{ entries.length - limit }})
      </button>
      <div class="modal-actions">
        <button class="button secondary" @click="emit('close')">{{ tr('Schließen') }}</button>
      </div>
    </div>
  </Modal>
</template>
