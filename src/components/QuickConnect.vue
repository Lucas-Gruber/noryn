<script setup lang="ts">
import { errorText, t as tr } from '../i18n';

import { computed, onBeforeUnmount, reactive, ref } from 'vue';
import { cancelHostKey, connectQuick, store } from '../composables/useNoryn';
import { clearCredential, emptyCredential } from '../connectionLogins';
import {
  defaultPort,
  isConnectionLink,
  parseConnectionLink,
  type ConnectionLink,
} from '../connectionLinks';
import type { NodeEntry } from '../types';
import Modal from './Modal.vue';
import SelectMenu from './SelectMenu.vue';
import Icon from './Icon.vue';
import ConnectionLoginFields from './ConnectionLoginFields.vue';
const props = defineProps<{ link?: ConnectionLink | null }>();
const emit = defineEmits<{ close: [] }>();
const node = reactive<NodeEntry>({
  id: crypto.randomUUID(),
  parentId: null,
  workspace: 'private',
  kind: 'connection',
  name: '',
  protocol: 'ssh',
  host: '',
  port: 22,
  initialPath: '',
  notes: '',
  tags: [],
  favorite: false,
});
function applyTarget(link: ConnectionLink) {
  const { protocol, host, port, initialPath } = link;
  Object.assign(node, { protocol, host, port, initialPath });
}
if (props.link) applyTarget(props.link);
const credential = reactive({ ...emptyCredential(), username: props.link?.username || '' });
const account = ref('direct');
const busy = ref(false),
  error = ref('');
const working = computed(() => busy.value || store.loading);
const ftp = computed(() => ['ftp', 'ftps'].includes(node.protocol));
function applyAddress(text = node.host) {
  if (!isConnectionLink(text)) return true;
  try {
    const link = parseConnectionLink(text);
    applyTarget(link);
    if (link.username) {
      credential.username = link.username;
      account.value = 'direct';
    }
    error.value = '';
    return true;
  } catch (cause) {
    error.value = String(cause);
    return false;
  }
}
function pasteAddress(event: ClipboardEvent) {
  const text = event.clipboardData?.getData('text') || '';
  if (isConnectionLink(text)) {
    event.preventDefault();
    applyAddress(text);
  }
}
function close() {
  if (!working.value) emit('close');
}
async function submit() {
  error.value = '';
  if (!applyAddress()) return;
  if (!node.host.trim() || !Number.isInteger(node.port) || node.port < 1 || node.port > 65535) {
    error.value = tr('Bitte eine Adresse und einen gültigen Port eingeben.');
    return;
  }
  if (
    account.value === 'direct' &&
    (!credential.username.trim() ||
      (!ftp.value &&
        node.protocol !== 'rdp' &&
        credential.kind === 'key' &&
        !credential.secret.trim()))
  ) {
    error.value = tr('Bitte Benutzername und gegebenenfalls einen privaten Schlüssel eingeben.');
    return;
  }
  busy.value = true;
  try {
    const request = {
      ...node,
      name: node.host.trim(),
      host: node.host.trim(),
      initialPath: ['ssh', 'rdp'].includes(node.protocol) ? '' : node.initialPath,
    };
    await connectQuick(
      request,
      {
        credentialId: account.value === 'direct' ? null : account.value,
        credential:
          account.value === 'direct'
            ? {
                ...credential,
                kind: ftp.value || node.protocol === 'rdp' ? 'password' : credential.kind,
              }
            : null,
      },
      () => emit('close'),
      () => emit('close'),
    );
  } catch (reason) {
    error.value = String(reason);
  } finally {
    clearCredential(credential);
    busy.value = false;
  }
}
onBeforeUnmount(() => {
  clearCredential(credential);
  if (store.challenge?.nodeId === node.id) void cancelHostKey();
});
</script>
<template>
  <Modal
    :title="tr('Schnell verbinden')"
    :subtitle="tr('Temporäre Sitzung · ohne Eintrag im Verbindungsbaum')"
    :busy="working"
    @close="close"
  >
    <form class="form-body" novalidate @submit.prevent="submit">
      <label
        >{{ tr('Protokoll')
        }}<SelectMenu
          v-model="node.protocol"
          :label="tr('Protokoll der Schnellverbindung')"
          :disabled="working"
          :options="[
            {
              value: 'ssh',
              get label() {
                return tr('SSH + SFTP');
              },
            },
            {
              value: 'sftp',
              get label() {
                return tr('SFTP · nur Dateien');
              },
            },
            {
              value: 'ftps',
              get label() {
                return tr('FTPS · TLS-verschlüsselt');
              },
            },
            {
              value: 'rdp',
              get label() {
                return tr('RDP · Remote-Desktop');
              },
            },
            {
              value: 'ftp',
              get label() {
                return tr('FTP · unverschlüsselt');
              },
            },
          ]"
          @update:model-value="node.port = defaultPort(node.protocol)"
      /></label>
      <div class="form-row host-row">
        <label
          >{{ tr('Adresse')
          }}<input
            v-model="node.host"
            :placeholder="tr('server.example.com')"
            maxlength="8192"
            @paste="pasteAddress"
            @blur="applyAddress()"
            :disabled="working"
            autofocus /></label
        ><label
          >{{ tr('Port')
          }}<input v-model.number="node.port" type="number" min="1" max="65535" :disabled="working"
        /></label>
      </div>
      <p class="muted">{{ tr('Auch als Link: ssh://benutzer@host:22 oder sftp://host/ordner') }}</p>
      <p v-if="node.protocol === 'rdp'" class="muted">
        {{
          tr(
            'RDP verwendet TLS und NLA. Als Benutzer sind auch DOMAIN\\Benutzer und benutzer@domain möglich.',
          )
        }}
      </p>
      <p v-if="node.protocol === 'ftp'" class="backup-error">
        {{
          tr(
            'FTP überträgt Zugangsdaten und Dateien unverschlüsselt. Verwende FTPS, wenn der Server TLS unterstützt.',
          )
        }}
      </p>
      <label v-if="['sftp', 'ftp', 'ftps'].includes(node.protocol)"
        >{{ tr('Startordner (optional)')
        }}<input
          v-model="node.initialPath"
          placeholder="/home/lucas"
          maxlength="4096"
          :disabled="working"
      /></label>
      <label
        >{{ tr('Zugang')
        }}<SelectMenu
          v-model="account"
          :label="tr('Zugang für die Schnellverbindung')"
          :disabled="working"
          :options="[
            {
              value: 'direct',
              get label() {
                return tr('Zugangsdaten jetzt eingeben');
              },
            },
            ...store.view.credentials.map((c) => ({
              value: c.id,
              label: c.name + ' · ' + c.username,
            })),
          ]"
      /></label>
      <ConnectionLoginFields
        v-if="account === 'direct'"
        :form="credential"
        :ftp="ftp || node.protocol === 'rdp'"
        :disabled="working"
      />
      <p class="muted">
        {{
          tr(
            'Die Sitzung ist als temporär gekennzeichnet. Du kannst sie später per Klick im privaten Tresor speichern.',
          )
        }}
      </p>
      <p v-if="error" class="backup-error" role="alert">{{ errorText(error) }}</p>
      <div class="modal-actions">
        <button type="button" class="button secondary" :disabled="working" @click="close">
          {{ tr('Abbrechen') }}</button
        ><button class="button primary" :disabled="working">
          <Icon :name="working ? 'LoaderCircle' : 'Play'" :class="{ spin: working }" :size="16" />{{
            working ? tr('Verbinde …') : tr('Verbinden')
          }}
        </button>
      </div>
    </form>
  </Modal>
</template>
