import type { decodeRdpFrame } from './rdpInput';

/** Pixel views stay on the received buffer; only ImageData/browser upload owns the next copy. */
export function paintRdpFrame(
  canvas: HTMLCanvasElement,
  context: CanvasRenderingContext2D,
  frame: ReturnType<typeof decodeRdpFrame>,
  createCanvas = () => document.createElement('canvas'),
) {
  if (canvas.width !== frame.width || canvas.height !== frame.height) {
    canvas.width = frame.width;
    canvas.height = frame.height;
  }
  if (frame.cursor?.kind === 'bitmap') {
    const cursor = frame.cursor;
    const shape = createCanvas();
    shape.width = cursor.width;
    shape.height = cursor.height;
    const target = shape.getContext('2d');
    if (target) {
      target.putImageData(new ImageData(cursor.pixels, cursor.width, cursor.height), 0, 0);
      // Only a locally encoded, bounded PNG is used as a native cursor. No server URLs or files.
      canvas.style.cursor = `url("${shape.toDataURL('image/png')}") ${cursor.hotX} ${cursor.hotY}, default`;
    } else canvas.style.cursor = 'default';
  } else if (frame.cursor)
    canvas.style.cursor = frame.cursor.kind === 'hidden' ? 'none' : 'default';
  for (const rect of frame.rectangles)
    context.putImageData(new ImageData(rect.pixels, rect.w, rect.h), rect.x, rect.y);
}
