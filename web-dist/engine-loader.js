import {threadCount, supportsThreads} from './thread-policy.js';
import {installThreadProxy, stopThreads} from './thread-proxy.js';

export async function loadEngine(action, progress) {
  const threads = threadCount(globalThis.navigator);
  if (action === 'prove' && threads > 1 && supportsThreads()) {
    try {
      installThreadProxy();
      const engine = await import('./engine-threaded/flight_core.js');
      await engine.default();
      await engine.initThreadPool(threads);
      postMessage({type: 'thread-ready'});
      progress(`使用 ${threads} 个计算线程…`);
      return engine;
    } catch (error) {
      stopThreads();
      console.warn('Parallel engine unavailable; using single-thread engine', error);
      progress('多线程不可用，使用单线程计算…');
    }
  }
  const engine = await import('./engine/flight_core.js');
  await engine.default();
  return engine;
}
