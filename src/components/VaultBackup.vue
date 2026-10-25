<script setup lang="ts">
import { errorText, t as tr } from '../i18n';

import { computed, onBeforeUnmount, ref, watch } from 'vue';
import { invoke } from '@tauri-apps/api/core';
import { open, save } from '@tauri-apps/plugin-dialog';
import { desktop, notify, store } from '../composables/useNoryn';
import type { VaultView } from '../types';
import Icon from './Icon.vue';
import Modal from './Modal.vue';
import SelectMenu from './SelectMenu.vue';

type Scope = { kind: 'all' } | { kind: 'private' } | { kind: 'team'; teamId: string };
interface Preview {
  id: string;
  summary: {
    scope: Scope;
    label: string;
    nodes: number;
    credentials: number;
    commands: number;
    scripts: number;
    teams: number;
    replacedNodes: number;
    replacedCredentials: number;
    replacedCommands: number;
    replacedScripts: number;
    sources: {
      workspace: string;
      name: string;
      nodes: number;
      credentials: number;
      commands: number;
      scripts: number;
      agentCredentials: number;
      inlineCredentials: number;
    }[];
  };
}
const mode = ref<'export' | 'import' | null>(null);
const selected = ref('all');
const password = ref('');
const repeat = ref('');
const memberName = ref('');
const busy = ref(false);
const error = ref('');
const file = ref('');
const preview = ref<Preview | null>(null);
const confirmed = ref(false);
const restoreTarget = '@original';
const importTarget = ref(restoreTarget);
const importSource = ref('private');
const includeCredentials = ref(false);
const restoring = computed(() => importTarget.value === restoreTarget);
const source = computed(() =>
  preview.value?.summary.sources.find((s) => s.workspace === importSource.value),
);
const sourceOptions = computed(
  () =>
    preview.value?.summary.sources.map((s) => ({
      value: s.workspace,
      label: tr('{p0} · {p1} Einträge · {p2} Befehle · {p3} Skripte', {
        p0: s.name,
        p1: s.nodes,
        p2: s.commands || 0,
        p3: s.scripts || 0,
      }),
    })) || [],
);
const targetOptions = computed(() => [
  {
    value: restoreTarget,
    get label() {
      return tr('Ursprünglichen Bereich wiederherstellen · ersetzen');
    },
  },
  {
    value: 'private',
    get label() {
      return tr('Privater Tresor · hinzufügen');
    },
  },
  ...store.view.teams.map((t) => ({
    value: t.id,
    label: tr('Team · {p0} · hinzufügen', { p0: t.name }),
  })),
]);
const targetTeam = computed(() => store.view.teams.find((t) => t.id === importTarget.value));
const importCredentialCount = computed(
  () =>
    (source.value?.credentials || 0) -
    (targetTeam.value ? source.value?.inlineCredentials || 0 : 0),
);
const needsMemberName = computed(
  () =>
    restoring.value &&
    preview.value?.summary.sources.some(
      (s) =>
        s.workspace !== 'private' &&
        !store.view.teams.find((t) => t.id === s.workspace)?.memberName.trim(),
    ),
);
const passwordShort = computed(
  () => mode.value === 'export' && password.value.length > 0 && [...password.value].length < 12,
);
watch([importTarget, importSource, includeCredentials], () => {
  confirmed.value = false;
  error.value = '';
});
watch([importTarget, importSource], () => {
  includeCredentials.value = false;
});
const options = computed(() => [
  {
    value: 'all',
    get label() {
      return tr('Alle Tresore · vollständige Sicherung');
    },
  },
  {
    value: 'private',
    get label() {
      return tr('Nur privater Tresor');
    },
  },
  ...store.view.teams.map((team) => ({
    value: `team:${team.id}`,
    label: tr('Team · {p0}', { p0: team.name }),
  })),
]);
const description = computed(() =>
  selected.value === 'all'
    ? tr(
        'Enthält private und Team-Einträge, Team-Skript-Sammlungen, alle Zugangsdaten, Team-Schlüssel und Server-Tokens sowie persönliche Zuordnungen, Ordner und Hostschlüssel.',
      )
    : selected.value === 'private'
      ? tr(
          'Enthält private Einträge, direkt hinterlegte Anmeldungen und Zugangsdaten mit persönlicher Ablage. Zuordnungen zu Konten anderer Tresore sind nicht enthalten.',
        )
      : tr(
          'Enthält den Team-Baum, die Skript-Sammlung, freigegebene Zugangsdaten, Verlauf sowie Team-Schlüssel und Server-Token. Persönliche Konten, Kontoauswahl, Namen und Ablage bleiben auf diesem Gerät.',
        ),
);
const sessionsOpen = computed(
  () => store.sessions.length > 0 || store.loading || !!store.authPrompt || !!store.challenge,
);
function start(value: 'export' | 'import') {
  mode.value = value;
  selected.value =
    store.workspace !== '*' && store.workspace !== 'private' ? `team:${store.workspace}` : 'all';
  error.value = '';
}
async function cancelPreview() {
  const id = preview.value?.id;
  preview.value = null;
  if (desktop && id) await invoke('vault_import_cancel', { id }).catch(() => {});
}
async function close() {
  if (busy.value) return;
  await cancelPreview();
  mode.value = null;
  password.value = repeat.value = memberName.value = file.value = error.value = '';
  confirmed.value = false;
  importTarget.value = restoreTarget;
  includeCredentials.value = false;
}
async function exportVault() {
  error.value = '';
  if ([...password.value].length < 12) {
    error.value = tr('Bitte mindestens 12 Zeichen für das Backup-Passwort verwenden.');
    return;
  }
  if (password.value !== repeat.value) {
    error.value = tr('Die Passwörter stimmen nicht überein.');
    return;
  }
  busy.value = true;
  try {
    const path = await save({
      defaultPath: `Noryn-${new Intl.DateTimeFormat('sv-SE').format(new Date())}.noryn`,
      filters: [
        {
          get name() {
            return tr('Noryn-Sicherung');
          },
          extensions: [tr('noryn')],
        },
      ],
    });
    if (!path) return;
    const scope: Scope =
      selected.value === 'all'
        ? { kind: 'all' }
        : selected.value === 'private'
          ? { kind: 'private' }
          : { kind: 'team', teamId: selected.value.slice(5) };
    await invoke('vault_export', { path, scope, password: password.value });
    busy.value = false;
    await close();
    notify(tr('Verschlüsselte Sicherung gespeichert'));
  } catch (e) {
    error.value = String(e);
  } finally {
    busy.value = false;
  }
}
async function selectFile() {
  const path = await open({
    multiple: false,
    directory: false,
    filters: [
      {
        get name() {
          return tr('Noryn-Sicherung');
        },
        extensions: [tr('noryn')],
      },
    ],
  });
  if (typeof path === 'string') file.value = path;
}
async function inspect() {
  if (!file.value || !password.value) {
    error.value = tr('Bitte eine Sicherungsdatei auswählen und das Backup-Passwort eingeben.');
    return;
  }
  busy.value = true;
  error.value = '';
  try {
    preview.value = await invoke<Preview>('vault_import_preview', {
      path: file.value,
      password: password.value,
    });
    password.value = '';
    confirmed.value = false;
    importTarget.value = restoreTarget;
    importSource.value = preview.value.summary.sources[0]?.workspace || 'private';
    includeCredentials.value = false;
    const scope = preview.value.summary.scope;
    memberName.value =
      scope.kind === 'team'
        ? store.view.teams.find((t) => t.id === scope.teamId)?.memberName || ''
        : '';
  } catch (e) {
    error.value = String(e);
  } finally {
    busy.value = false;
  }
}
async function applyImport() {
  if (!preview.value || !confirmed.value) return;
  busy.value = true;
  error.value = '';
  if (needsMemberName.value && !memberName.value.trim()) {
    error.value = tr('Bitte deinen Namen für importierte Teams eingeben.');
    busy.value = false;
    return;
  }
  try {
    store.view = await invoke<VaultView>('vault_import_apply', {
      id: preview.value.id,
      memberName: memberName.value,
      destination: restoring.value
        ? null
        : {
            sourceWorkspace: importSource.value,
            targetWorkspace: importTarget.value,
            includeCredentials: includeCredentials.value,
          },
    });
    preview.value = null;
    store.selected = '';
    store.folder = 'all';
    if (
      store.workspace !== '*' &&
      store.workspace !== 'private' &&
      !store.view.teams.some((t) => t.id === store.workspace)
    )
      store.workspace = 'private';
    busy.value = false;
    await close();
    notify(tr('Sicherung importiert. Team-Änderungen sind zunächst lokal.'));
  } catch (e) {
    error.value = String(e);
  } finally {
    busy.value = false;
  }
}
onBeforeUnmount(() => {
  password.value = repeat.value = '';
  void cancelPreview();
});
</script>

<template>
  <section class="settings-section" aria-labelledby="backup-heading">
    <div class="settings-section-head">
      <div>
        <h2 id="backup-heading">{{ tr('Tresore sichern & übertragen') }}</h2>
        <p>{{ tr('Verschlüsselter Export und Import für deine privaten und Team-Tresore.') }}</p>
      </div>
      <Icon name="ShieldCheck" :size="24" />
    </div>
    <p class="backup-description">
      {{
        tr(
          'Schütze jede Sicherung mit einem eigenen Passwort. Du kannst alle Tresore gemeinsam oder einen einzelnen Bereich exportieren.',
        )
      }}
    </p>
    <div class="backup-actions">
      <button class="button secondary" @click="start('export')">
        <Icon name="Download" :size="17" />{{ tr('Exportieren') }}
      </button>
      <button class="button secondary" @click="start('import')">
        <Icon name="Upload" :size="17" />{{ tr('Importieren') }}
      </button>
    </div>
    <p v-if="!desktop" class="muted">
      {{
        tr(
          'Export und Import sind in der Desktop-App verfügbar. Die Browser-Vorschau enthält nur Beispieldaten.',
        )
      }}
    </p>
  </section>
  <Modal
    v-if="mode"
    :title="mode === 'export' ? tr('Tresor exportieren') : tr('Sicherung importieren')"
    :subtitle="tr('Noryn-Sicherung · Argon2id + XChaCha20-Poly1305')"
    @close="close"
  >
    <form
      class="form-body"
      novalidate
      @submit.prevent="mode === 'export' ? exportVault() : preview ? applyImport() : inspect()"
    >
      <template v-if="mode === 'export'">
        <label
          >{{ tr('Bereich')
          }}<SelectMenu
            v-model="selected"
            :options="options"
            :label="tr('Exportbereich')"
            :disabled="busy"
        /></label>
        <p class="backup-description">{{ description }}</p>
        <label
          >{{ tr('Backup-Passwort')
          }}<input
            v-model="password"
            type="password"
            autocomplete="new-password"
            required
            aria-describedby="backup-password-help"
            :aria-invalid="passwordShort"
            :disabled="busy"
        /></label>
        <p
          id="backup-password-help"
          :class="passwordShort ? 'backup-error' : 'muted'"
          role="status"
        >
          {{ tr('Mindestens 12 Zeichen')
          }}<span v-if="passwordShort"> {{ tr('· aktuell') }} {{ [...password].length }}</span
          >.
        </p>
        <label
          >{{ tr('Passwort wiederholen')
          }}<input
            v-model="repeat"
            type="password"
            autocomplete="new-password"
            required
            :disabled="busy"
        /></label>
        <p class="muted">
          {{
            tr(
              'Das Backup-Passwort wird nicht gespeichert. Bewahre es sicher auf. Vorhandene Dateien werden nicht überschrieben.',
            )
          }}
        </p>
      </template>
      <template v-else-if="!preview">
        <label
          >{{ tr('Sicherungsdatei')
          }}<button
            type="button"
            class="button secondary backup-file"
            :disabled="busy || !desktop"
            @click="selectFile().catch((e) => (error = String(e)))"
          >
            <Icon name="FolderOpen" :size="17" />{{ file || tr('Datei auswählen …') }}
          </button></label
        >
        <label
          >{{ tr('Backup-Passwort')
          }}<input v-model="password" type="password" autocomplete="off" required :disabled="busy"
        /></label>
        <p class="muted">
          {{
            tr(
              'Zuerst wird die Sicherung entschlüsselt und geprüft. Dabei wird noch nichts geändert.',
            )
          }}
        </p>
      </template>
      <template v-else>
        <div class="backup-summary">
          <strong>{{ preview.summary.label }}</strong
          ><span
            >{{ preview.summary.nodes }} {{ tr('Einträge ·') }} {{ preview.summary.credentials }}
            {{ tr('Konten ·') }} {{ preview.summary.teams }} {{ tr('Teams ·') }}
            {{ preview.summary.commands || 0 }} {{ tr('Befehle ·') }}
            {{ preview.summary.scripts || 0 }} {{ tr('Skripte') }}</span
          >
        </div>
        <label
          >{{ tr('Import-Ziel')
          }}<SelectMenu
            v-model="importTarget"
            :options="targetOptions"
            :label="tr('Import-Ziel')"
            :disabled="busy"
        /></label>
        <label v-if="!restoring && sourceOptions.length > 1"
          >{{ tr('Bereich aus der Sicherung')
          }}<SelectMenu
            v-model="importSource"
            :options="sourceOptions"
            :label="tr('Quell-Tresor')"
            :disabled="busy"
        /></label>
        <p v-if="restoring" class="backup-description">
          {{ tr('Der gesicherte Bereich ersetzt dort') }} {{ preview.summary.replacedNodes }}
          {{ tr('vorhandene Einträge,') }} {{ preview.summary.replacedCredentials }}
          {{ tr('Konten und') }} {{ preview.summary.replacedCommands || 0 }} {{ tr('Befehle und') }}
          {{ preview.summary.replacedScripts || 0 }} {{ tr('Skripte.') }}
          {{
            preview.summary.scope.kind === 'all'
              ? tr('Alle Tresore werden wiederhergestellt.')
              : tr('Andere Tresore bleiben erhalten.')
          }}
          {{ tr('Dein aktuelles Tresor-Passwort bleibt bestehen.') }}
        </p>
        <template v-else>
          <p class="backup-description">
            {{ source?.nodes || 0 }} {{ tr('Einträge und') }} {{ source?.commands || 0 }}
            {{ tr('Befehle aus „') }}{{ source?.name
            }}{{
              tr('“ werden mit neuen IDs hinzugefügt. Vorhandene Einträge werden nicht ersetzt.')
            }}
          </p>
          <p v-if="source?.scripts" class="muted">
            {{
              targetTeam
                ? tr(
                    '{p0} Skripte und ihre Ordner werden mit neuen IDs in die Team-Sammlung übernommen. Der Skript-Abgleich bleibt bis zur manuellen Bestätigung angehalten.',
                    { p0: source.scripts },
                  )
                : tr(
                    '{p0} Team-Skripte werden übersprungen: Im privaten Tresor gibt es keine Skript-Sammlung.',
                    { p0: source.scripts },
                  )
            }}
          </p>
          <label class="checkbox-row"
            ><input
              v-model="includeCredentials"
              type="checkbox"
              :disabled="busy || !importCredentialCount"
            />{{
              targetTeam
                ? tr('Zugangsdaten übernehmen und in diesem Team freigeben')
                : tr('Zugangsdaten in den privaten Tresor übernehmen')
            }}
            ({{ importCredentialCount }})</label
          >
          <p v-if="targetTeam" class="security-note">
            {{ tr('Verbindungsdaten, Kategorien und Notizen werden mit „') }}{{ targetTeam.name
            }}{{
              tr(
                '“ geteilt. Mit aktiviertem Häkchen werden auch die enthaltenen Passwörter und Schlüssel für dieses Team freigegeben.',
              )
            }}
          </p>
          <p v-if="targetTeam && source?.agentCredentials" class="muted">
            {{ source.agentCredentials }}
            {{
              tr(
                'SSH-Agent-Zugänge werden nicht in den Team-Tresor übernommen, da sie an das lokale Gerät gebunden sind.',
              )
            }}
          </p>
          <p v-if="targetTeam && source?.inlineCredentials" class="muted">
            {{ source.inlineCredentials }}
            {{
              tr(
                'direkt in privaten Verbindungen hinterlegte Anmeldungen bleiben privat und werden nicht in das Team übernommen.',
              )
            }}
          </p>
          <p class="muted">
            {{
              tr(
                'Persönliche Kontoauswahl, Ablage, Hostschlüssel, bisheriger Verlauf und Quell-Team-Konfiguration werden nicht übernommen. Wähle die Konten anschließend am Ziel aus.',
              )
            }}
          </p>
          <p v-if="targetTeam && !targetTeam.memberName.trim()" class="backup-error">
            {{ tr('Bitte zuerst unter Team-Sync deinen Anzeigenamen für dieses Team festlegen.') }}
          </p>
        </template>
        <label v-if="needsMemberName"
          >{{ tr('Dein Name für importierte Teams')
          }}<input v-model="memberName" maxlength="80" required :disabled="busy" /><span
            class="muted"
            >{{
              tr(
                'Bestehende persönliche Team-Namen bleiben erhalten. Der Import wird im Team-Verlauf erfasst.',
              )
            }}</span
          ></label
        >
        <p class="muted">
          {{ restoring ? tr('Zuordnungen zu entfernten Konten werden bereinigt.') : '' }}
          {{
            tr(
              'Die Vorschau gilt fünf Minuten. Live-Sync wird für importierte Teams ausgeschaltet. Team-Änderungen bleiben bis zum bewussten Abgleich lokal. SSH-Agent-Konten verwenden den Agenten dieses Geräts.',
            )
          }}
        </p>
        <p v-if="sessionsOpen" class="backup-error">
          {{ tr('Bitte zuerst alle Verbindungen und Anmeldungen schließen.') }}
        </p>
        <label class="checkbox-row"
          ><input v-model="confirmed" type="checkbox" :disabled="busy" />{{
            restoring
              ? tr('Ich möchte den angezeigten Bereich durch die Sicherung ersetzen.')
              : tr('Ich möchte die ausgewählten Daten im Ziel-Tresor hinzufügen.')
          }}</label
        >
      </template>
      <p v-if="error" class="backup-error" role="alert">{{ errorText(error) }}</p>
      <div class="modal-actions">
        <button type="button" class="button secondary" :disabled="busy" @click="close">
          {{ tr('Abbrechen') }}
        </button>
        <button
          class="button primary"
          :disabled="
            busy ||
            !desktop ||
            (mode === 'import' &&
              (!file ||
                (!!preview &&
                  (!confirmed || sessionsOpen || (!!targetTeam && !targetTeam.memberName.trim())))))
          "
        >
          <Icon v-if="busy" name="LoaderCircle" :size="16" />{{
            mode === 'export'
              ? tr('Sicherung speichern')
              : preview
                ? restoring
                  ? tr('Bereich wiederherstellen')
                  : tr('Im Ziel hinzufügen')
                : tr('Vorschau öffnen')
          }}
        </button>
      </div>
    </form>
  </Modal>
</template>
