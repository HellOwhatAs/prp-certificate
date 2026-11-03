// Match the native default: use all available logical CPUs reported by the host.
// Browsers may report fewer CPUs than the operating system exposes to native code.
export function threadCount({hardwareConcurrency = 1} = {}) {
  const cores = Number.isFinite(hardwareConcurrency) ? Math.floor(hardwareConcurrency) : 1;
  return Math.max(1, cores);
}

export function supportsThreads() {
  if (!globalThis.crossOriginIsolated || typeof SharedArrayBuffer === 'undefined') return false;
  try {
    return new WebAssembly.Memory({initial: 1, maximum: 1, shared: true}).buffer instanceof SharedArrayBuffer;
  } catch { return false; }
}
