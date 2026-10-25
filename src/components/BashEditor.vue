<script setup lang="ts">
import { onMounted, onBeforeUnmount, ref, watch } from 'vue';
import { basicSetup, EditorView } from 'codemirror';
import { StreamLanguage, syntaxHighlighting, HighlightStyle } from '@codemirror/language';
import { shell } from '@codemirror/legacy-modes/mode/shell';
import { tags } from '@lezer/highlight';
const props = defineProps<{ modelValue: string; readonly?: boolean }>();
const emit = defineEmits<{ 'update:modelValue': [value: string] }>();
const element = ref<HTMLElement>();
let editor: EditorView | undefined;
onMounted(() => {
  editor = new EditorView({
    parent: element.value,
    doc: props.modelValue,
    extensions: [
      basicSetup,
      StreamLanguage.define(shell),
      syntaxHighlighting(
        HighlightStyle.define([
          { tag: tags.keyword, color: 'var(--accent)' },
          { tag: [tags.string, tags.special(tags.string)], color: 'var(--success)' },
          { tag: tags.comment, color: 'var(--muted)', fontStyle: 'italic' },
          { tag: [tags.number, tags.bool], color: 'var(--warning)' },
        ]),
      ),
      EditorView.editable.of(!props.readonly),
      EditorView.contentAttributes.of({ 'aria-label': 'Bash-Skript', spellcheck: 'false' }),
      EditorView.updateListener.of((update) => {
        if (update.docChanged) emit('update:modelValue', update.state.doc.toString());
      }),
    ],
  });
});
watch(
  () => props.modelValue,
  (value) => {
    if (editor && editor.state.doc.toString() !== value)
      editor.dispatch({ changes: { from: 0, to: editor.state.doc.length, insert: value } });
  },
);
onBeforeUnmount(() => editor?.destroy());
</script>
<template><div ref="element" class="bash-editor" /></template>
<style>
.bash-editor {
  border: 1px solid var(--border);
  border-radius: 8px;
  overflow: hidden;
}
.bash-editor .cm-editor {
  background: var(--bg);
  color: var(--text);
  font-size: 1rem;
}
.bash-editor .cm-scroller {
  font-family: 'JetBrains Mono Variable', monospace;
  height: clamp(16rem, 44vh, 36rem);
  overflow: auto;
  line-height: 1.65;
}
.bash-editor .cm-gutters {
  background: var(--surface);
  color: var(--muted);
  border-right-color: var(--border);
}
.bash-editor .cm-activeLine,
.bash-editor .cm-activeLineGutter {
  background: color-mix(in srgb, var(--accent) 7%, transparent);
}
.bash-editor .cm-cursor {
  border-left-color: var(--text);
}
.bash-editor .cm-selectionBackground,
.bash-editor .cm-editor.cm-focused .cm-selectionBackground {
  background: color-mix(in srgb, var(--accent) 35%, transparent) !important;
}
.bash-editor .cm-focused {
  outline: 2px solid var(--accent);
  outline-offset: -2px;
}
.bash-editor .cm-panels {
  background: var(--surface);
  color: var(--text);
}
</style>
