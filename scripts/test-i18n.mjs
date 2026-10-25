import assert from 'node:assert/strict';
import { createRequire } from 'node:module';
import { readFile, readdir, mkdtemp, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { createServer } from 'vite';
import { createSSRApp, computed, nextTick } from 'vue';
import { EditorState, Compartment } from '@codemirror/state';
import { renderToString } from 'vue/server-renderer';
const require = createRequire(import.meta.url);
const { parse } = require('vue/compiler-sfc');
const ts = require('typescript');
const root = fileURLToPath(new URL('..', import.meta.url));
const catalog = JSON.parse(await readFile(join(root, 'src/locales/en.json'), 'utf8'));
const parameters = (text) => [...new Set(text.match(/\{[a-zA-Z][a-zA-Z0-9_]*\}/g) || [])].sort();
for (const [source, translated] of Object.entries(catalog)) {
  assert.equal(typeof translated, 'string');
  assert.ok(translated.length, source);
  assert.deepEqual(parameters(translated), parameters(source), `Translation parameters: ${source}`);
}
let keys = 0;
function checkExpressions(source, file) {
  const tree = ts.createSourceFile(file, source, ts.ScriptTarget.Latest, true);
  function visit(node) {
    if (
      ts.isCallExpression(node) &&
      ['tr', 't'].includes(node.expression.getText(tree)) &&
      ts.isStringLiteral(node.arguments[0])
    ) {
      const key = node.arguments[0].text;
      assert.ok(Object.hasOwn(catalog, key), `Missing translation in ${file}: ${key}`);
      keys++;
    }
    ts.forEachChild(node, visit);
  }
  visit(tree);
}
for (const file of (await readdir(join(root, 'src'), { recursive: true })).filter((f) =>
  /\.(vue|ts)$/.test(f),
)) {
  const source = await readFile(join(root, 'src', file), 'utf8');
  if (file.endsWith('.ts')) {
    checkExpressions(source, file);
    continue;
  }
  const { descriptor } = parse(source);
  if (descriptor.scriptSetup) checkExpressions(descriptor.scriptSetup.content, file);
  function walk(node, skip = false) {
    if (
      node.type === 2 &&
      !skip &&
      node.content.trim().length > 1 &&
      /[A-Za-zÄÖÜäöüß]/.test(node.content)
    )
      assert.fail(`Untranslated static UI text in ${file}: ${node.content.trim()}`);
    if (node.type === 5) checkExpressions(node.content.content, file);
    if (node.type === 1)
      for (const prop of node.props) {
        if (
          prop.type === 6 &&
          ['title', 'label', 'subtitle', 'aria-label', 'placeholder'].includes(prop.name) &&
          prop.value &&
          !/^(?:https?:|\/|--)/.test(prop.value.content) &&
          !prop.value.content.startsWith('-----BEGIN ') &&
          /[äöüÄÖÜß]|[A-Za-z]{3}/.test(prop.value.content)
        )
          assert.fail(`Untranslated UI attribute in ${file}: ${prop.value.content}`);
        if (prop.type === 7 && prop.exp) checkExpressions(prop.exp.content, file);
      }
    for (const child of node.children || [])
      walk(child, skip || ['pre', 'code', 'svg'].includes(node.tag));
  }
  if (descriptor.template) walk(descriptor.template.ast);
}
const cacheDir = await mkdtemp(join(tmpdir(), 'noryn-i18n-'));
const previousDocument = globalThis.document,
  previousStorage = globalThis.localStorage;
const memory = new Map();
globalThis.localStorage = {
  getItem: (key) => memory.get(key) || null,
  setItem: (key, value) => memory.set(key, value),
};
globalThis.document = { documentElement: { dataset: {}, style: {} } };
const server = await createServer({
  root,
  cacheDir,
  optimizeDeps: { noDiscovery: true, include: [] },
  server: { middlewareMode: true, hmr: false, watch: null },
});
try {
  const {
    language,
    locale,
    systemLanguage,
    t,
    messageText,
    errorText,
    formatLocale,
    editorPhrases,
  } = await server.ssrLoadModule('/src/i18n.ts');
  assert.equal(language.value, 'system');
  assert.equal(systemLanguage(['de-AT', 'en-US']), 'de');
  assert.equal(systemLanguage(['en-GB', 'de-DE']), 'en');
  assert.equal(systemLanguage(['fr-FR', 'de-DE']), 'de');
  assert.equal(systemLanguage(['fr-FR']), 'en');
  const settings = (await server.ssrLoadModule('/src/components/SettingsPage.vue')).default;
  const { store } = await server.ssrLoadModule('/src/composables/useNoryn.ts');
  const before = JSON.stringify(store.view);
  const { themes } = await server.ssrLoadModule('/src/composables/useAppearance.ts');
  const name = computed(() => themes[0].name);
  language.value = 'de';
  await nextTick();
  const german = await renderToString(createSSRApp(settings));
  assert.match(german, /Darstellung zurücksetzen/);
  assert.match(german, /Programme für Dateien/);
  assert.equal(name.value, 'Graphit');
  const editorLanguage = new Compartment();
  let editor = EditorState.create({
    doc: 'printf hello',
    extensions: [editorLanguage.of(EditorState.phrases.of(editorPhrases()))],
  });
  assert.equal(editor.phrase('Find'), 'Suchen');
  assert.equal(formatLocale(), 'de-DE');
  language.value = 'en';
  await nextTick();
  const english = await renderToString(createSSRApp(settings));
  assert.match(english, /Reset appearance/);
  assert.match(english, /Programs for files/);
  assert.match(english, /aria-label="Language"/);
  assert.equal(t('Systemsprache'), 'System language');
  assert.match(english, /Updates are available in the desktop app/);
  assert.doesNotMatch(english, /Darstellung zurücksetzen|Verbindung bereit|Updates sind/);
  assert.equal(t('{p0} Verbindung|{p0} Verbindungen', { p0: 1 }, 1), '1 connection');
  assert.equal(t('{p0} Ordner', { p0: 3 }, 3), '3 folders');
  assert.equal(name.value, 'Graphite');
  editor = editor.update({
    effects: editorLanguage.reconfigure(EditorState.phrases.of(editorPhrases())),
  }).state;
  assert.equal(editor.phrase('Find'), 'Find');
  assert.equal(editor.doc.toString(), 'printf hello');
  assert.equal(formatLocale(), 'en-US');
  assert.equal(document.documentElement.lang, 'en');
  assert.equal(memory.get('noryn.locale.v1'), '"en"');
  assert.equal(
    JSON.stringify(store.view),
    before,
    'Language must never change vault, user names or team data',
  );
  assert.equal(
    t('{p0} schließen', { p0: 'Bücher {p1} /tmp/a;$(x)' }),
    'Close Bücher {p1} /tmp/a;$(x)',
  );
  assert.equal(
    messageText('Updates sind in der Desktop-App verfügbar.'),
    'Updates are available in the desktop app.',
  );
  assert.equal(
    errorText('SSH-Handshake fehlgeschlagen: original diagnostic'),
    'SSH handshake failed: original diagnostic',
  );
  language.value = 'de';
  assert.equal(
    messageText('Updates are available in the desktop app.'),
    'Updates sind in der Desktop-App verfügbar.',
  );
  language.value = 'system';
  assert.equal(memory.get('noryn.locale.v1'), '"system"');
  assert.equal(locale.value, systemLanguage(navigator.languages));
  console.log(
    `i18n: ${Object.keys(catalog).length} messages, ${keys} source calls, placeholder parity, full settings in both languages, system preference, persistence, reactivity and vault isolation passed.`,
  );
} finally {
  await server.close();
  await rm(cacheDir, { recursive: true, force: true });
  globalThis.document = previousDocument;
  globalThis.localStorage = previousStorage;
}
