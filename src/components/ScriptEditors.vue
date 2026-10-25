<script setup lang="ts">
import { errorText, t as tr } from '../i18n';

import { ref } from 'vue';
import type { ScriptInfo } from '../types';
import {
  scriptEditors,
  openScriptEditor,
  finishScriptEditor,
  discardScriptEditor,
} from '../composables/useScriptEditors';
import Modal from './Modal.vue';
import Icon from './Icon.vue';
const error = ref('');
const discard = ref<(typeof scriptEditors)[number]>();
async function open(teamId: string, entry: ScriptInfo) {
  error.value = '';
  try {
    await openScriptEditor(teamId, entry);
  } catch (e) {
    error.value = String(e);
  }
}
defineExpose({ open });
</script>
<template>
  <p v-if="error" class="error-message" role="alert">{{ errorText(error) }}</p>
  <article
    v-for="editor in scriptEditors"
    :key="editor.id"
    class="script-editor-status tools-notice"
  >
    <div>
      <strong><Icon name="ExternalLink" :size="15" /> {{ editor.name }}</strong>
      <p>{{ editor.editorNote }}</p>
      <p v-if="editor.error" class="error-message" role="alert">{{ errorText(editor.error) }}</p>
    </div>
    <div class="command-actions">
      <button
        class="button secondary small"
        :disabled="editor.busy || editor.autoFinish"
        @click="finishScriptEditor(editor)"
      >
        {{ tr('Bearbeitung beenden') }}</button
      ><button class="button secondary small" :disabled="editor.busy" @click="discard = editor">
        {{ tr('Verwerfen …') }}
      </button>
    </div>
  </article>
  <Modal
    v-if="discard"
    :title="tr('Arbeitskopie verwerfen?')"
    :subtitle="discard.name"
    @close="discard = undefined"
    ><div class="form-body">
      <p>
        {{
          tr(
            'Die lokale Arbeitskopie wird entfernt. Nicht übernommene Änderungen gehen verloren. Schließe das Dokument zuerst im externen Editor.',
          )
        }}
      </p>
      <div class="modal-actions">
        <button class="button secondary" @click="discard = undefined">{{ tr('Abbrechen') }}</button
        ><button
          class="button primary"
          @click="
            discardScriptEditor(discard);
            discard = undefined;
          "
        >
          {{ tr('Verwerfen') }}
        </button>
      </div>
    </div></Modal
  >
</template>
<style>
.script-editor-status {
  display: flex;
  justify-content: space-between;
  gap: 14px;
  flex-wrap: wrap;
}
.script-editor-status p {
  margin: 8px 0;
}
.script-editor-status strong {
  display: flex;
  align-items: center;
  gap: 8px;
}
</style>
