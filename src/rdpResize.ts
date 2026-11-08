export function rdpResolution(width: number, height: number) {
  if (!Number.isFinite(width) || !Number.isFinite(height) || width <= 0 || height <= 0) return null;
  return {
    width: Math.max(200, Math.min(3840, Math.floor(width))) & ~1,
    height: Math.max(200, Math.min(2160, Math.floor(height))),
  };
}
type Resolution = NonNullable<ReturnType<typeof rdpResolution>>;

/** One resize in flight, trailing debounce, newest viewport wins. No polling or hidden-tab requests. */
export function createRdpResizer(options: {
  active: () => boolean;
  measure: () => Resolution | null;
  send: (size: Resolution) => Promise<unknown>;
  failed: (error: unknown) => void;
  schedule?: (callback: () => void) => () => void;
}) {
  let disposed = false,
    busy = false;
  let cancel: (() => void) | undefined;
  let last: Resolution | undefined;
  const same = (a: Resolution | null | undefined, b: Resolution | null | undefined) =>
    a?.width === b?.width && a?.height === b?.height;
  const schedule =
    options.schedule ||
    ((callback) => {
      const id = setTimeout(callback, 300);
      return () => clearTimeout(id);
    });
  async function flush() {
    cancel = undefined;
    if (disposed || busy || !options.active()) return;
    const size = options.measure();
    if (!size || same(size, last)) return;
    busy = true;
    let success = false;
    try {
      await options.send(size);
      if (!disposed) last = size;
      success = true;
    } catch (error) {
      if (!disposed) options.failed(error);
    } finally {
      busy = false;
      if (success && !disposed && options.active() && !same(options.measure(), last)) request();
    }
  }
  function request() {
    cancel?.();
    cancel = undefined;
    if (!disposed && options.active())
      cancel = schedule(() => {
        void flush();
      });
  }
  return {
    request,
    reset() {
      last = undefined;
      request();
    },
    dispose() {
      disposed = true;
      cancel?.();
      cancel = undefined;
    },
  };
}
