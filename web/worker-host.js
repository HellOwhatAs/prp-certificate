// Own every Rayon worker at page level: terminate/cancel also releases its memory.
export class EngineWorker {
  constructor() {
    this.children = new Map();
    this.worker = new Worker(new URL('./zk-worker.js', import.meta.url), {type: 'module'});
    this.worker.onmessage = event => {
      if (this.terminated) return;
      const data = event.data;
      if (data.type === 'thread-spawn') {
        try {
          const child = new Worker(data.url, data.options);
          this.children.set(data.id, child);
          child.onmessage = e => this.worker.postMessage({type: 'thread-message', id: data.id, data: e.data});
          child.onerror = e => {
            if (this.poolReady) this.onerror?.(e);
            else this.worker.postMessage({type: 'thread-error', id: data.id, message: e.message});
          };
        } catch (e) {
          this.worker.postMessage({type: 'thread-error', id: data.id, message: String(e)});
        }
      } else if (data.type === 'thread-ready') {
        this.poolReady = true;
      } else if (data.type === 'thread-send') {
        try { this.children.get(data.id)?.postMessage(data.data); }
        catch (e) { this.worker.postMessage({type: 'thread-error', id: data.id, message: String(e)}); }
      } else if (data.type === 'thread-stop') {
        this.children.get(data.id)?.terminate();
        this.children.delete(data.id);
      } else {
        this.onmessage?.(event);
      }
    };
    this.worker.onerror = e => this.onerror?.(e);
  }
  postMessage(data) { this.worker.postMessage(data); }
  terminate() {
    this.terminated = true;
    this.worker.terminate();
    for (const child of this.children.values()) child.terminate();
    this.children.clear();
  }
}
