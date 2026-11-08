import assert from 'node:assert/strict';

export async function checkRdpDisplay(server) {
  const { scanCode, decodeRdpFrame } = await server.ssrLoadModule('/src/rdpInput.ts');
  const { paintRdpFrame } = await server.ssrLoadModule('/src/rdpDisplay.ts');
  assert.equal(scanCode('ControlRight'), 0x11d);
  assert.equal(scanCode('KeyA'), 0x1e);
  assert.equal(scanCode('Unidentified'), undefined);

  function packet(rectangles, cursor = null) {
    const bitmap = cursor?.kind === 'bitmap';
    const bytes = new ArrayBuffer(
      24 +
        (bitmap ? cursor.pixels.length : 0) +
        rectangles.reduce((n, r) => n + 8 + r.w * r.h * 4, 0),
    );
    const view = new DataView(bytes);
    view.setUint32(0, 0x4e524432, false);
    view.setUint32(4, 17, true);
    for (const [offset, value] of [
      [8, 640],
      [10, 480],
      [12, rectangles.length],
      [14, !cursor ? 0 : bitmap ? 3 : cursor.kind === 'hidden' ? 2 : 1],
      [16, bitmap ? cursor.width : 0],
      [18, bitmap ? cursor.height : 0],
      [20, bitmap ? cursor.hotX : 0],
      [22, bitmap ? cursor.hotY : 0],
    ])
      view.setUint16(offset, value, true);
    let offset = 24;
    if (bitmap) {
      new Uint8Array(bytes, offset, cursor.pixels.length).set(cursor.pixels);
      offset += cursor.pixels.length;
    }
    for (const r of rectangles) {
      for (const [i, n] of [r.x, r.y, r.w, r.h].entries()) view.setUint16(offset + i * 2, n, true);
      offset += 8;
      new Uint8Array(bytes, offset, r.w * r.h * 4).set(r.pixels);
      offset += r.w * r.h * 4;
    }
    return bytes;
  }
  const source = packet(
    [
      { x: 10, y: 20, w: 1, h: 1, pixels: [40, 90, 180, 255] },
      { x: 630, y: 470, w: 1, h: 1, pixels: [100, 110, 120, 255] },
    ],
    { kind: 'hidden' },
  );
  const frame = decodeRdpFrame(source);
  assert.equal(frame.sequence, 17);
  assert.equal(frame.cursor.kind, 'hidden');
  assert.equal(frame.rectangles.length, 2);
  assert.deepEqual([...frame.rectangles[0].pixels], [40, 90, 180, 255]);
  assert.equal(
    frame.rectangles[1].pixels.buffer,
    source,
    'all rectangles reference the original received buffer',
  );
  for (const length of [0, 23, 24, source.byteLength - 1])
    assert.throws(() => decodeRdpFrame(source.slice(0, length)));
  function corrupted(offset, value) {
    const bytes = source.slice(0);
    new DataView(bytes).setUint16(offset, value, true);
    return bytes;
  }
  for (const [offset, value] of [
    [8, 0],
    [8, 65535],
    [10, 65535],
    [12, 2041],
    [14, 4],
    [16, 1],
    [24, 640],
    [28, 0],
    [30, 65535],
  ])
    assert.throws(() => decodeRdpFrame(corrupted(offset, value)));
  assert.throws(
    () => decodeRdpFrame(packet([])),
    'empty unchanged packets must not cause render/ACK churn',
  );

  const shape = {
    kind: 'bitmap',
    width: 2,
    height: 1,
    hotX: 1,
    hotY: 0,
    pixels: [1, 2, 3, 255, 4, 5, 6, 255],
  };
  const cursorPacket = packet([], shape);
  assert.equal(cursorPacket.byteLength, 32, 'a cursor change must not contain desktop pixels');
  assert.equal(decodeRdpFrame(cursorPacket).rectangles.length, 0);
  for (const [offset, value] of [
    [16, 129],
    [18, 0],
    [20, 2],
    [22, 1],
  ]) {
    const bytes = cursorPacket.slice(0);
    new DataView(bytes).setUint16(offset, value, true);
    assert.throws(() => decodeRdpFrame(bytes));
  }

  const original = globalThis.ImageData;
  try {
    globalThis.ImageData = class {
      constructor(data, width, height) {
        this.data = data;
        this.width = width;
        this.height = height;
      }
    };
    const calls = [],
      context = { putImageData: (...args) => calls.push(args) };
    const canvas = { width: 640, height: 480, style: { cursor: 'default' } };
    paintRdpFrame(canvas, context, frame);
    assert.equal(canvas.style.cursor, 'none');
    assert.equal(calls.length, 2);
    assert.equal(
      calls[0][0].data,
      frame.rectangles[0].pixels,
      'ImageData must not duplicate the full pixel buffer',
    );
    assert.deepEqual(
      calls.map((c) => c.slice(1)),
      [
        [10, 20],
        [630, 470],
      ],
      'separate patches keep their positions',
    );
    let shapes = 0;
    paintRdpFrame(canvas, context, decodeRdpFrame(cursorPacket), () => {
      shapes++;
      return {
        getContext: () => context,
        toDataURL: (mime) => {
          assert.equal(mime, 'image/png');
          return 'data:image/png;base64,test-only';
        },
      };
    });
    assert.equal(shapes, 1);
    assert.equal(canvas.style.cursor, 'url("data:image/png;base64,test-only") 1 0, default');
    const customCursor = canvas.style.cursor;
    paintRdpFrame(
      canvas,
      context,
      decodeRdpFrame(packet([{ x: 0, y: 0, w: 1, h: 1, pixels: [0, 0, 0, 255] }])),
    );
    assert.equal(
      canvas.style.cursor,
      customCursor,
      'ordinary image updates must not rebuild the cursor',
    );
    paintRdpFrame(canvas, context, decodeRdpFrame(packet([], { kind: 'default' })));
    assert.equal(canvas.style.cursor, 'default');
  } finally {
    globalThis.ImageData = original;
  }
  console.log(
    'RDP display: bounded multi-region packets, zero-copy pixel views, independent local cursor and malformed payload rejection passed.',
  );
}
