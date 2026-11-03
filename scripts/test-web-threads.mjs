// Browser integration / optional full proof benchmark. Requires Playwright + Chromium.
// No answers or certificates are written to the report; --job stays local.
import fs from 'node:fs/promises';
import {createReadStream} from 'node:fs';
import http from 'node:http';
import path from 'node:path';
import assert from 'node:assert/strict';
const {chromium} = await import(process.env.PLAYWRIGHT_MODULE || 'playwright');
const argument = (name, fallback) => { const i = process.argv.indexOf(name); return i < 0 ? fallback : process.argv[i + 1]; };
const jobPath = argument('--job', null);
const benchmarkThreads = Number(argument('--threads', 4));
assert.ok(Number.isInteger(benchmarkThreads) && benchmarkThreads > 1);
const benchmarkModes = argument('--modes', 'parallel,single').split(',');
assert.ok(benchmarkModes.every(mode => ['parallel', 'single'].includes(mode)));
const reportPath = argument('--report', '.cache/web-thread-benchmark.json');
const root = path.resolve('dist/client');
const prefix = '/prp-certificate/';
const manifest = JSON.parse(await fs.readFile('public/groth16/manifest.json'));
const parameters = new Map(Object.values(manifest.circuits).flatMap(info => info.chunks.map(p => [`parameters/${info.pk_sha256}/${p.file}`, path.resolve('public/groth16', p.file)])));
let mode = 'parallel';
const types = {'.js':'text/javascript', '.wasm':'application/wasm', '.html':'text/html', '.json':'application/json'};
const server = http.createServer(async (req, res) => {
  try {
    const pathname = new URL(req.url, 'http://localhost').pathname;
    if (!pathname.startsWith(prefix)) throw Error('outside project');
    const relative = decodeURIComponent(pathname.slice(prefix.length));
    if (relative === 'probe.html') {
      res.writeHead(200, {'content-type':'text/html','cache-control':'no-store'});
      res.end(`<script type="module">import {prepareIsolation} from './isolation.js';await prepareIsolation();await import('./bridge.js');window.ready=true;</script>`);
      return;
    }
    if (mode === 'broken' && relative.includes('workerHelpers')) throw Error('simulated thread startup failure');
    const file = parameters.get(relative) || path.resolve(root, relative || 'index.html');
    if (!parameters.has(relative) && !file.startsWith(root + path.sep)) throw Error('outside root');
    // Control only test worker hardware, keeping shipping code free of test switches.
    if (relative === 'zk-worker.js') {
      const source = await fs.readFile(file, 'utf8');
      res.writeHead(200, {'content-type':'text/javascript','cache-control':'no-store'});
      res.end(`Object.defineProperty(navigator,'hardwareConcurrency',{value:${mode === 'single' ? 1 : mode === 'many-cores' ? 16 : benchmarkThreads}});\n${source}`);
      return;
    }
    const size = (await fs.stat(file)).size;
    res.writeHead(200, {'content-type':types[path.extname(file)] || 'application/octet-stream','content-length':size,'cache-control':'no-store'});
    createReadStream(file).pipe(res);
  } catch { res.writeHead(404); res.end('not found'); }
});
await new Promise(resolve => server.listen(0, '127.0.0.1', resolve));
const url = `http://127.0.0.1:${server.address().port}${prefix}`;
const browser = await chromium.launch({headless:true, executablePath:process.env.CHROMIUM_PATH || undefined, args:['--no-sandbox','--disable-dev-shm-usage']});
const invalid = {action:'prove',q:1,team:'test',files:{routes:'invalid',assignments:'invalid'}};
const results = [];
async function open(blocked = false) {
  const context = await browser.newContext({serviceWorkers:blocked ? 'block' : 'allow'});
  const page = await context.newPage();
  page.on('console', message => { if (['error','warning'].includes(message.type())) console.error(message.text()); else if(message.text().startsWith('proof-step')) console.log(message.text()); });
  await page.addInitScript(failChild => {
    window.liveWorkers = new Set();
    const NativeWorker = Worker;
    let children = 0;
    window.Worker = class extends NativeWorker {
      constructor(...args) {
        if (failChild && String(args[0]).includes('workerHelpers') && ++children === 2) throw Error('simulated child startup failure');
        super(...args); liveWorkers.add(this);
      }
      terminate() { super.terminate(); liveWorkers.delete(this); }
    };
  }, mode === 'broken-child');
  await page.goto(url + 'probe.html');
  await page.waitForFunction(() => window.ready, null, {timeout:20000});
  assert.equal(await page.evaluate(() => crossOriginIsolated), !blocked);
  return {context, page};
}
async function run(page, job) {
  const result = await page.evaluate(async job => {
    const started = performance.now(), progress = [];
    let beats = 0;
    const heartbeat = setInterval(() => beats++, 50);
    flightBridge.send(JSON.stringify(job));
    try {
      for (;;) {
        for (const message of JSON.parse(flightBridge.drain())) {
          if (message.type === 'progress') { progress.push({ms:performance.now() - started,text:message.text}); if(!/MB|缓存/.test(message.text)) console.log('proof-step', ((performance.now()-started)/1000).toFixed(1), message.text); }
          else return {message,progress,ms:performance.now() - started,beats,live:liveWorkers.size};
        }
        if (performance.now() - started > 900000) throw Error('Proof timeout');
        await new Promise(resolve => setTimeout(resolve, 30));
      }
    } finally { clearInterval(heartbeat); flightBridge.cancel(); }
  }, job);
  assert.equal(result.live, 0, 'all workers terminated before final result');
  return result;
}
try {
  for (const test of ['parallel','many-cores','single','broken','broken-child','blocked']) {
    mode = test;
    const {context,page} = await open(test === 'blocked');
    const result = await run(page, invalid);
    assert.equal(result.message.type, 'error');
    assert.match(result.message.message, /CSV|表头|字段|航线|列/);
    if (test === 'parallel' || test === 'many-cores') assert.ok(result.progress.some(p => p.text.includes(`使用 ${test === 'many-cores' ? 16 : benchmarkThreads} 个`)), JSON.stringify(result));
    else assert.ok(!result.progress.some(p => /使用 \d+ 个/.test(p.text)), JSON.stringify(result));
    if (test.startsWith('broken')) assert.ok(result.progress.some(p => /多线程不可用/.test(p.text)));
    console.log(`${test}: isolation, Worker startup, invalid-input rejection and cleanup passed`);
    await context.close();
  }
  if (jobPath) {
    const job = JSON.parse(await fs.readFile(jobPath));
    let expected;
    for (const test of benchmarkModes) {
      mode = test;
      const {context,page} = await open();
      // Cancel a live pool, then prove again in the same page.
      if (test === 'parallel') {
        await page.evaluate(job => flightBridge.send(JSON.stringify(job)), job);
        await page.waitForFunction(threads => liveWorkers.size === threads + 1, benchmarkThreads);
        await page.evaluate(() => flightBridge.cancel());
        assert.equal(await page.evaluate(() => liveWorkers.size), 0);
      }
      const value = await run(page, {...job,action:'prove'});
      assert.equal(value.message.type, 'result', JSON.stringify(value.message));
      assert.ok(value.beats > 10, 'page remains responsive during proof');
      const certificate = value.message.result.certificate;
      const verified = await run(page, {action:'verify',certificate});
      assert.equal(verified.message.type, 'result');
      assert.deepEqual(verified.message.result.scores, value.message.result.scores);
      if (expected) assert.deepEqual(value.message.result.scores, expected);
      expected = value.message.result.scores;
      const forged = structuredClone(certificate);
      forged.certificates[0].scores[0] += 1;
      assert.equal((await run(page,{action:'verify',certificate:forged})).message.type, 'error');
      const {message,...timing} = value;
      results.push({mode:test,threads:test === 'parallel' ? benchmarkThreads : 1,...timing});
      await fs.mkdir(path.dirname(reportPath),{recursive:true});
      await fs.writeFile(reportPath, JSON.stringify({browser:browser.version(),results},null,2));
      console.log(`${test}: ${(value.ms/1000).toFixed(2)} s; verified; tampering rejected; UI heartbeat ${value.beats}`);
      await context.close();
    }
  }
} finally { await browser.close(); await new Promise(resolve => server.close(resolve)); }
