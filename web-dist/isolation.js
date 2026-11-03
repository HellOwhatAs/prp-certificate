// GitHub Pages cannot set COOP/COEP headers. A scoped service worker supplies
// them on the next navigation; try once, before the user has entered any data.
export async function prepareIsolation() {
  const key = 'flight-isolation-reload';
  if (globalThis.crossOriginIsolated) {
    try { sessionStorage.removeItem(key); } catch {}
    return;
  }
  if (!globalThis.isSecureContext || !navigator.serviceWorker) return;
  try {
    await Promise.race([
      (async () => {
        await navigator.serviceWorker.register(new URL('./isolation-worker.js', import.meta.url), {scope: './'});
        await navigator.serviceWorker.ready;
        if (!navigator.serviceWorker.controller) {
          await new Promise(resolve => navigator.serviceWorker.addEventListener('controllerchange', resolve, {once: true}));
        }
      })(),
      new Promise((_, reject) => setTimeout(() => reject(new Error('isolation timeout')), 3000))
    ]);
    if (!sessionStorage.getItem(key)) {
      sessionStorage.setItem(key, '1');
      location.reload();
      // Do not start the UI before the pending reload.
      await new Promise(() => {});
    }
  } catch (error) { console.warn('Cross-origin isolation unavailable; single-thread fallback', error); }
}
