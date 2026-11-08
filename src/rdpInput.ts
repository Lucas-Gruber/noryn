/** RDP set-1 scan codes, including the E0 extended bit. Values never form commands. */
const codes: Record<string, number> = {
  Escape: 1,
  Digit1: 2,
  Digit2: 3,
  Digit3: 4,
  Digit4: 5,
  Digit5: 6,
  Digit6: 7,
  Digit7: 8,
  Digit8: 9,
  Digit9: 10,
  Digit0: 11,
  Minus: 12,
  Equal: 13,
  Backspace: 14,
  Tab: 15,
  KeyQ: 16,
  KeyW: 17,
  KeyE: 18,
  KeyR: 19,
  KeyT: 20,
  KeyY: 21,
  KeyU: 22,
  KeyI: 23,
  KeyO: 24,
  KeyP: 25,
  BracketLeft: 26,
  BracketRight: 27,
  Enter: 28,
  ControlLeft: 29,
  KeyA: 30,
  KeyS: 31,
  KeyD: 32,
  KeyF: 33,
  KeyG: 34,
  KeyH: 35,
  KeyJ: 36,
  KeyK: 37,
  KeyL: 38,
  Semicolon: 39,
  Quote: 40,
  Backquote: 41,
  ShiftLeft: 42,
  Backslash: 43,
  KeyZ: 44,
  KeyX: 45,
  KeyC: 46,
  KeyV: 47,
  KeyB: 48,
  KeyN: 49,
  KeyM: 50,
  Comma: 51,
  Period: 52,
  Slash: 53,
  ShiftRight: 54,
  NumpadMultiply: 55,
  AltLeft: 56,
  Space: 57,
  CapsLock: 58,
  F1: 59,
  F2: 60,
  F3: 61,
  F4: 62,
  F5: 63,
  F6: 64,
  F7: 65,
  F8: 66,
  F9: 67,
  F10: 68,
  NumLock: 69,
  ScrollLock: 70,
  Numpad7: 71,
  Numpad8: 72,
  Numpad9: 73,
  NumpadSubtract: 74,
  Numpad4: 75,
  Numpad5: 76,
  Numpad6: 77,
  NumpadAdd: 78,
  Numpad1: 79,
  Numpad2: 80,
  Numpad3: 81,
  Numpad0: 82,
  NumpadDecimal: 83,
  IntlBackslash: 86,
  F11: 87,
  F12: 88,
  NumpadEnter: 0x11c,
  ControlRight: 0x11d,
  NumpadDivide: 0x135,
  PrintScreen: 0x137,
  AltRight: 0x138,
  Home: 0x147,
  ArrowUp: 0x148,
  PageUp: 0x149,
  ArrowLeft: 0x14b,
  ArrowRight: 0x14d,
  End: 0x14f,
  ArrowDown: 0x150,
  PageDown: 0x151,
  Insert: 0x152,
  Delete: 0x153,
  MetaLeft: 0x15b,
  MetaRight: 0x15c,
  ContextMenu: 0x15d,
};
export function scanCode(code: string) {
  return codes[code];
}
export type RdpCursor =
  | { kind: 'default' | 'hidden' }
  | {
      kind: 'bitmap';
      width: number;
      height: number;
      hotX: number;
      hotY: number;
      pixels: Uint8ClampedArray<ArrayBuffer>;
    };
export function decodeRdpFrame(buffer: ArrayBuffer) {
  const invalid = () => {
    throw Error('Invalid RDP frame');
  };
  const maxPixels = 3840 * 2160;
  if (buffer.byteLength < 24 || buffer.byteLength > 24 + maxPixels * 4 + 2040 * 8 + 128 * 128 * 4)
    return invalid();
  const view = new DataView(buffer);
  if (view.getUint32(0, false) !== 0x4e524432) return invalid();
  const sequence = view.getUint32(4, true);
  const width = view.getUint16(8, true),
    height = view.getUint16(10, true);
  const count = view.getUint16(12, true),
    kind = view.getUint16(14, true);
  const cw = view.getUint16(16, true),
    ch = view.getUint16(18, true),
    hotX = view.getUint16(20, true),
    hotY = view.getUint16(22, true);
  if (
    !width ||
    !height ||
    width > 3840 ||
    height > 2160 ||
    count > 2040 ||
    kind > 3 ||
    (!count && !kind)
  )
    return invalid();
  let offset = 24;
  let cursor: RdpCursor | null = null;
  if (kind === 3) {
    if (
      !cw ||
      !ch ||
      cw > 128 ||
      ch > 128 ||
      hotX >= cw ||
      hotY >= ch ||
      offset + cw * ch * 4 > buffer.byteLength
    )
      return invalid();
    cursor = {
      kind: 'bitmap',
      width: cw,
      height: ch,
      hotX,
      hotY,
      pixels: new Uint8ClampedArray(buffer, offset, cw * ch * 4),
    };
    offset += cw * ch * 4;
  } else {
    if (cw || ch || hotX || hotY) return invalid();
    if (kind) cursor = { kind: kind === 1 ? 'default' : 'hidden' };
  }
  const rectangles: {
    x: number;
    y: number;
    w: number;
    h: number;
    pixels: Uint8ClampedArray<ArrayBuffer>;
  }[] = [];
  let area = 0;
  for (let index = 0; index < count; index++) {
    if (offset + 8 > buffer.byteLength) return invalid();
    const x = view.getUint16(offset, true),
      y = view.getUint16(offset + 2, true),
      w = view.getUint16(offset + 4, true),
      h = view.getUint16(offset + 6, true);
    offset += 8;
    area += w * h;
    if (
      !w ||
      !h ||
      x + w > width ||
      y + h > height ||
      area > width * height ||
      offset + w * h * 4 > buffer.byteLength
    )
      return invalid();
    rectangles.push({ x, y, w, h, pixels: new Uint8ClampedArray(buffer, offset, w * h * 4) });
    offset += w * h * 4;
  }
  if (offset !== buffer.byteLength) return invalid();
  return { sequence, width, height, cursor, rectangles };
}
