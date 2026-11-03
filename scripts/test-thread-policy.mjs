import assert from 'node:assert/strict';
import {threadCount} from '../web/thread-policy.js';
for (const [hardwareConcurrency, deviceMemory, expected] of [
  [1, 8, 1], [2, 8, 2], [4, 8, 4], [8, 8, 8], [16, 8, 16],
  [32, 4, 32], [32, 2, 32], [NaN, 8, 1], [0, 8, 1], [64, undefined, 64],
  [undefined, undefined, 1], [Infinity, 8, 1], [-2, 8, 1]
]) assert.equal(threadCount({hardwareConcurrency, deviceMemory}), expected);
assert.equal(threadCount(), 1);

// Page-level ownership must stop every thread on completion, cancellation and failure.
const workers = [];
globalThis.Worker = class {
  constructor(url) { this.url = url; this.sent = []; workers.push(this); }
  postMessage(data) { this.sent.push(data); }
  terminate() { this.stopped = true; }
};
const {EngineWorker} = await import('../web/worker-host.js');
const host = new EngineWorker(), main = workers[0];
main.onmessage({data: {type: 'thread-spawn', id: 1, url: 'http://localhost/thread.js'}});
main.onmessage({data: {type: 'thread-spawn', id: 2, url: 'http://localhost/thread.js'}});
main.onmessage({data: {type: 'thread-send', id: 1, data: {value: 42}}});
assert.deepEqual(workers[1].sent, [{value: 42}]);
workers[1].onmessage({data: {ready: true}});
assert.deepEqual(main.sent, [{type: 'thread-message', id: 1, data: {ready: true}}]);
let error;
host.onerror = e => error = e.message;
main.onmessage({data: {type: 'thread-ready'}});
workers[2].onerror({message: 'runtime failure'});
assert.equal(error, 'runtime failure');
host.terminate();
assert.ok(workers.every(worker => worker.stopped));
assert.equal(host.children.size, 0);
console.log('Adaptive thread policy, message routing, runtime errors and cancellation passed');
