// Verify the actual Pages artifact, including project-site paths and Raw routing.
import fs from 'node:fs/promises';
import assert from 'node:assert/strict';
import {parameterParts} from '../dist/client/parameters.js';
const root='dist/client';
const deployment=JSON.parse(await fs.readFile(`${root}/deployment.json`));
const manifest=JSON.parse(await fs.readFile(`${root}/groth16/manifest.json`));
assert.equal(deployment.parameterBaseUrl,`https://raw.githubusercontent.com/${deployment.repository}/${deployment.ref}/public/groth16/`);
let chunks=0;
for(let q=1;q<=3;q++){
  const info=manifest.circuits[q];
  const parts=parameterParts(q,info);
  assert.equal(parts.length,info.chunks.length);
  for(const part of parts){
    assert.equal(part.url,`${deployment.parameterBaseUrl}${part.file}`);
    assert.equal(new URL(part.url).protocol,'https:');
    chunks++;
  }
}
let bytes=0;
async function walk(dir){
  for(const entry of await fs.readdir(dir,{withFileTypes:true})){
    const path=`${dir}/${entry.name}`;
    if(entry.isDirectory())await walk(path);
    else{
      assert.ok(!path.endsWith('.dat'),'large parameters must not be in the Pages artifact');
      bytes+=(await fs.stat(path)).size;
    }
  }
}
await walk(root);
assert.ok(bytes<100_000_000,`unexpectedly large static site: ${bytes}`);
for(const file of ['index.html','bridge.js','zk-worker.js','ui/flight_ui.js','engine/flight_core.js','ui/flight_ui_bg.wasm','engine/flight_core_bg.wasm','engine-threaded/flight_core.js','engine-threaded/flight_core_bg.wasm','worker-host.js','engine-loader.js','thread-policy.js','thread-proxy.js','isolation.js','isolation-worker.js','.nojekyll'])
  await fs.access(`${root}/${file}`);
const index=await fs.readFile(`${root}/index.html`,'utf8');
assert.ok(index.includes("import './bridge.js'"));
assert.ok(index.includes("from './ui/flight_ui.js'"));
assert.ok(!/(?:src|href)=["']\//.test(index),'project Pages must use relative paths');
assert.ok((await fs.readFile(`${root}/worker-host.js`,'utf8')).includes("new URL('./zk-worker.js', import.meta.url)"));
assert.match(await fs.readFile(`${root}/engine-threaded/flight_core.js`,'utf8'), /shared:\s*true/);
console.log(`Pages passed: ${bytes} bytes, ${chunks} pinned parameter URLs, relative assets and worker path`);
