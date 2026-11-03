// Bundler-free wasm-bindgen-rayon 1.3 worker adapter. Same-origin module URL;
// fail explicitly on startup errors and release all workers on fallback.
function ready(worker, init) {
  return new Promise((resolve, reject) => {
    const timer = setTimeout(() => finish(new Error('计算线程启动超时')), 15000);
    const message = event => {
      if (event.data?.type === 'rayon-ready') finish();
      if (event.data?.type === 'rayon-error') finish(new Error(event.data.message));
    };
    const error = event => finish(new Error(event.message || '计算线程启动失败'));
    function finish(reason) {
      clearTimeout(timer);
      worker.removeEventListener('message', message);
      worker.removeEventListener('error', error);
      reason ? reject(reason) : resolve();
    }
    worker.addEventListener('message', message);
    worker.addEventListener('error', error);
    worker.postMessage(init);
  });
}

const workers = [];
export async function startWorkers(module, memory, builder) {
  try {
    const init = {type: 'rayon-init', module, memory, receiver: builder.receiver(), mainJS: builder.mainJS()};
    await Promise.all(Array.from({length: builder.numThreads()}, () => {
      const worker = new Worker(import.meta.url, {type: 'module'});
      workers.push(worker);
      return ready(worker, init);
    }));
    builder.build();
  } catch (error) {
    for (const worker of workers) worker.terminate();
    workers.length = 0;
    throw error;
  }
}

globalThis.addEventListener?.('message', async ({data}) => {
  if (data?.type !== 'rayon-init') return;
  try {
    const engine = await import(data.mainJS);
    await engine.default({module_or_path: data.module, memory: data.memory});
    postMessage({type: 'rayon-ready'});
    engine.wbg_rayon_start_worker(data.receiver);
  } catch (error) { postMessage({type: 'rayon-error', message: String(error)}); }
});
