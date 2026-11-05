// Keep PDF rasterization off the UI thread. The document is embedded in the same
// Rust module as the app, so no PDF/font/CDN requests are needed to read a page.
let ready;
async function renderer() {
  if (!ready) {
    ready = (async () => {
      const codec = await import('./ui/flight_ui.js');
      await codec.default();
      return codec;
    })().catch(error => { ready = null; throw error; });
  }
  return ready;
}

globalThis.onmessage = async ({data}) => {
  const {id, page, width} = data || {};
  let initialized = false;
  try {
    if (!Number.isSafeInteger(id) || !Number.isSafeInteger(page) || page < 0 ||
        !Number.isSafeInteger(width) || width < 1 || width > 4096) {
      throw Error('PDF 显示请求无效');
    }
    const codec = await renderer();
    initialized = true;
    const bytes = codec.render_problem_pdf_page(page, width);
    postMessage({id, bytes}, [bytes.buffer]);
  } catch (error) {
    // A fresh worker is needed after a failed module import (ESM caches failures)
    // or a trapped WASM instance. Ordinary page errors can reuse the renderer.
    postMessage({id, error: error?.message || String(error),
      fatal: !initialized || error instanceof WebAssembly.RuntimeError});
  }
};
