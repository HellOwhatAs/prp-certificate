// Exact deployed import Worker: success must return the original proven claims.
import fs from 'node:fs/promises';
import assert from 'node:assert/strict';
let result;
globalThis.postMessage=m=>{if(m.type!=='progress')result=m;};globalThis.onmessage=null;
globalThis.fetch=async url=>new Response(await fs.readFile(new URL(url)),{headers:{'content-type':String(url).endsWith('.wasm')?'application/wasm':'application/octet-stream'}});
await import('../dist/client/zk-worker.js');
const expected=JSON.parse(await fs.readFile(process.argv[2]));
const cases=JSON.parse(await fs.readFile('.cache/groth16-images/results.json')).filter(c=>c.case!=='pdf-rejected').map(c=>[c.case+(c.case.includes('jpeg')?'.jpg':'.png'),c.recovered]);
for(const [name,ok] of cases){
 const start=performance.now();const bytes=await fs.readFile(`.cache/groth16-images/${name}`);
 await globalThis.onmessage({data:{action:'import',file:bytes.toString('base64')}});
 assert.equal(result.type,ok?'result':'error',`${name}: ${JSON.stringify(result)}`);
 if(ok)assert.deepEqual(result.result,expected);
 console.log(`${name}: ${result.type}; ${((performance.now()-start)/1000).toFixed(2)} s`);
}
