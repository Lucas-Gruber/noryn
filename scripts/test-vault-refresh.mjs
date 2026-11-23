import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';

const app = readFileSync(new URL('../src/App.vue', import.meta.url), 'utf8');
const backend = readFileSync(new URL('../src-tauri/src/lib.rs', import.meta.url), 'utf8');
const startup = app.slice(app.indexOf('onMounted(() =>'), app.indexOf('onBeforeUnmount('));
assert.ok(startup.includes("await invoke('vault_lock');"), 'refresh must lock native vault');
assert.ok(startup.indexOf("await invoke('vault_lock');") < startup.indexOf("invoke<boolean>('vault_exists')"), 'lock before startup initialization');
assert.ok(startup.includes('store.loading = true;') && startup.includes('store.loading = false;'), 'block unlock during initialization');
assert.ok(app.includes(':disabled="store.loading"'), 'unlock submit must be disabled while initialization runs');
assert.ok(backend.includes('fn vault_lock('), 'native lock command must exist');
console.log('Vault refresh structural regression checks passed (5 assertions)');
