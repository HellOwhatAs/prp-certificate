// Exact deployed import Worker: success must return the original proven claims.
import fs from 'node:fs/promises';
import assert from 'node:assert/strict';
let result;
globalThis.postMessage=m=>{if(m.type!=='progress')result=m;};globalThis.onmessage=null;
globalThis.fetch=async url=>new Response(await fs.readFile(new URL(url)),{headers:{'content-type':String(url).endsWith('.wasm')?'application/wasm':'application/octet-stream'}});
await import('../dist/client/zk-worker.js');
const expected=JSON.parse(await fs.readFile(process.argv[2]));
const cases=[['original.png',true],['jpeg80.jpg',true],['resize-1280-jpeg80.jpg',true],['resize-1200-jpeg80.jpg',true],['resize-800-jpeg80.jpg',true],['resize-640-jpeg80.jpg',false],['changed-text.png',true],['removed-qr.png',false],['grayscale.png',true],['rotated.png',true],['cropped.png',true],['qr-only.png',true]];
for(const [name,ok] of cases){
 const start=performance.now();const bytes=await fs.readFile(`.cache/groth16-images/${name}`);
 await globalThis.onmessage({data:{action:'import',file:bytes.toString('base64')}});
 assert.equal(result.type,ok?'result':'error',`${name}: ${JSON.stringify(result)}`);
 if(ok)assert.deepEqual(result.result,expected);
 console.log(`${name}: ${result.type}; ${((performance.now()-start)/1000).toFixed(2)} s`);
}
