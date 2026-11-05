/** Preserve special bits when editing rwx; never invent modes absent from server metadata. */
export function parseMode(value: string): number | undefined {
  return /^[0-7]{3,4}$/.test(value) ? Number.parseInt(value, 8) : undefined;
}
export function formatMode(mode: number): string {
  return mode.toString(8).padStart(3, '0');
}
export function parseId(value: string): number | undefined {
  if (!/^\d{1,10}$/.test(value)) return undefined;
  const id = Number(value);
  return id < 0xffffffff ? id : undefined;
}
export function symbolicMode(mode: number): string {
  const text = Array.from({ length: 9 }, (_, index) =>
    mode & (1 << (8 - index)) ? 'rwx'[index % 3]! : '-',
  );
  for (const [index, bit, letter] of [
    [2, 0o4000, 's'],
    [5, 0o2000, 's'],
    [8, 0o1000, 't'],
  ] as const) {
    if (mode & bit) text[index] = text[index] === 'x' ? letter : letter.toUpperCase();
  }
  return text.join('');
}
