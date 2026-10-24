<script setup lang="ts">
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
  created: 'Angelegt',
  updated: 'Geändert',
  deleted: 'Entfernt',
  shared: 'Freigegeben',
  unshared: 'Freigabe aufgehoben',
  imported: 'Importiert',
};
const objects: Record<TeamChange['object'], string> = {
  folder: 'Ordner',
  connection: 'Verbindung',
  credential: 'Zugangsdaten',
  workspace: 'Tresor',
};
const fields: Record<ChangedField, string> = {
  name: 'Name',
  parent: 'Übergeordneter Ordner',
  kind: 'Typ',
  host: 'Host',
  port: 'Port',
  protocol: 'Protokoll',
  initialPath: 'Startordner',
  notes: 'Notizen',
  tags: 'Kategorien',
  favorite: 'Favorit',
  username: 'Benutzername',
  privateKey: 'Privater Schlüssel',
  passphrase: 'Schlüssel-Passphrase',
  password: 'Benutzerpasswort',
  children: 'Enthaltene Einträge',
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
            .toLocaleLowerCase()
            .includes(query.value.trim().toLocaleLowerCase())),
    ),
);
const rows = computed(() => entries.value.slice(0, limit.value));
const date = new Intl.DateTimeFormat('de-DE', { dateStyle: 'medium', timeStyle: 'medium' });
watch([query, actionFilter], () => (limit.value = 50));
</script>
<template>
  <Modal title="Änderungsverlauf" :subtitle="team.name" wide @close="emit('close')">
    <div class="form-body team-activity">
      <p v-if="!desktop" class="activity-preview">Browser-Vorschau · Beispieldaten</p>
      <p>
        Wer hat was geändert? Die letzten 1.000 Änderungen werden mit dem Team verschlüsselt
        synchronisiert. Namen werden von den Mitgliedern selbst angegeben.
      </p>
      <p class="activity-sync-note">
        {{
          team.liveSync
            ? 'Bei aktivem Live-Sync erscheinen Änderungen automatisch. Konflikte werden unter Team-Sync angezeigt.'
            : team.dirty
              ? 'Enthält lokale Änderungen. Andere sehen sie nach deinem nächsten Upload.'
              : 'Neue Änderungen anderer Mitglieder erscheinen nach dem Abrufen vom Server.'
        }}
      </p>
      <div class="activity-filters">
        <label class="search-field"
          ><Icon name="Search" :size="16" /><input
            v-model="query"
            aria-label="Änderungsverlauf durchsuchen"
            placeholder="Name oder Eintrag suchen …"
        /></label>
        <SelectMenu
          v-model="actionFilter"
          label="Änderungen filtern"
          :options="[
            { value: 'all', label: 'Alle Änderungen' },
            ...Object.entries(actions).map(([value, label]) => ({ value, label })),
          ]"
        />
      </div>
      <div v-if="rows.length" class="activity-table-wrap">
        <table class="activity-table">
          <thead>
            <tr>
              <th>Wann</th>
              <th>Wer</th>
              <th>Änderung</th>
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
            ? 'Keine passenden Änderungen.'
            : 'Noch keine protokollierten Änderungen. Ältere Änderungen werden nicht nachträglich zugeordnet.'
        }}
      </p>
      <button v-if="entries.length > limit" class="button secondary" @click="limit += 50">
        Weitere Änderungen anzeigen ({{ entries.length - limit }})
      </button>
      <div class="modal-actions">
        <button class="button secondary" @click="emit('close')">Schließen</button>
      </div>
    </div>
  </Modal>
</template>
