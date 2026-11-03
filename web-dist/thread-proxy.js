// Minimal Worker interface used by wasm-bindgen-rayon, routed through the page.
const proxies = new Map();
let nextId = 0;
export function installThreadProxy() {
  globalThis.Worker = class extends EventTarget {
    constructor(url, options) {
      super();
      this.id = ++nextId;
      proxies.set(this.id, this);
      postMessage({type: 'thread-spawn', id: this.id, url: String(url), options});
    }
    postMessage(data) { postMessage({type: 'thread-send', id: this.id, data}); }
    terminate() { postMessage({type: 'thread-stop', id: this.id}); proxies.delete(this.id); }
  };
  addEventListener('message', event => {
    const data = event.data;
    if (!['thread-message', 'thread-error'].includes(data?.type)) return;
    event.stopImmediatePropagation();
    const proxy = proxies.get(data.id);
    proxy?.dispatchEvent(data.type === 'thread-message'
      ? new MessageEvent('message', {data: data.data})
      : new ErrorEvent('error', {message: data.message}));
  });
}
export function stopThreads() {
  for (const proxy of proxies.values()) proxy.terminate();
}
