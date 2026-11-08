import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';

const source = readFileSync(new URL('../src/components/ConnectionTree.vue', import.meta.url), 'utf8');
const dragStart = source.match(/function startDrag\([\s\S]*?\n}\nfunction endDrag\(/)?.[0];
assert.ok(dragStart, 'connection drag handler exists');
assert.match(dragStart, /setData\('application\/noryn-node', node\.id\)/);
assert.match(dragStart, /createElement\('canvas'\)/);
assert.match(dragStart, /preview\.width = 1;/);
assert.match(dragStart, /preview\.height = 1;/);
assert.match(dragStart, /setDragImage\(preview, 0, 0\)/);
assert.doesNotMatch(dragStart, /node\.name|connection-drag-preview|cloneNode/);
assert.match(dragStart, /setTimeout\(clearDragPreview, 0\)/);
console.log('Connection drag preview regression assertions passed');
