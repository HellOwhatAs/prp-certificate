// Exercise the exact published Worker and WASM codecs without requiring a GPU.
// Node is a test host only; no Node module is imported by the published files.
import fs from 'node:fs/promises';
import assert from 'node:assert/strict';
let result;
globalThis.postMessage=message=>{if(message.type!=='progress')result=message;};
globalThis.onmessage=null;
globalThis.fetch=async url=>new Response(await fs.readFile(new URL(url)),{
 headers:{'content-type':String(url).endsWith('.wasm')?'application/wasm':'application/octet-stream'}
});
await import('../dist/client/zk-worker.js');
const codec=await import('../dist/client/ui/flight_ui.js');
await codec.default();
for(const path of process.argv.slice(2)){
 const original=JSON.parse(await fs.readFile(path));
 await globalThis.onmessage({data:{action:'verify',certificate:original.certificate}});
 assert.equal(result.type,'result',JSON.stringify(result));
 assert.deepEqual(result.result.scores,original.scores);
 const item=JSON.stringify(result.result),png=codec.certificate_png(item);
 const nativePng=path.replace(/\.json$/,'.png');
 await fs.writeFile(nativePng,png);
 assert.deepEqual(JSON.parse(codec.parse_certificate_file(png)),original.certificate);
 await globalThis.onmessage({data:{action:'import',file:Buffer.from(png).toString('base64')}});
 assert.equal(result.type,'result',JSON.stringify(result));assert.deepEqual(result.result.scores,original.scores);
 const forged=structuredClone(original.certificate);forged.certificates[0].scores[0]+=1;
 const forgedImage=codec.certificate_png(JSON.stringify({...original,certificate:forged,scores:forged.certificates.map(c=>c.scores)}));
 await globalThis.onmessage({data:{action:'import',file:Buffer.from(forgedImage).toString('base64')}});
 assert.equal(result.type,'error');
 console.log(`${path}: published Worker verifies; Groth16 QR PNG import passes; image containing a forged score proof is rejected`);
}
await globalThis.onmessage({data:{action:'invalid'}});assert.equal(result.type,'error');
