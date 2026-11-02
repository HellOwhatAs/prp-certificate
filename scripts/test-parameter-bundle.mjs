import fs from 'node:fs/promises';
import {createHash} from 'node:crypto';
import assert from 'node:assert/strict';
import {readParameterBundle} from '../web/parameter-bundle.js';
const manifest=JSON.parse(await fs.readFile('public/groth16/manifest.json'));
for(let q=1;q<=3;q++){
 let file=new File([await fs.readFile(`release/prp-flight-parameters-q${q}.tar.gz`)],`parameters-q${q}.tar.gz`);
 let bytes=await readParameterBundle(file,manifest.circuits[q]);
 assert.equal(bytes.length,manifest.circuits[q].pk_bytes);
 assert.equal(createHash('sha256').update(bytes).digest('hex'),manifest.circuits[q].pk_sha256);
 console.log(`Q${q}: streamed bundle reconstructs exact pinned key (${bytes.length} bytes)`);
 bytes=null;
 await assert.rejects(()=>readParameterBundle(file,manifest.circuits[q===1?2:1]));
 await assert.rejects(()=>readParameterBundle(file.slice(0,3000),manifest.circuits[q]));
 file=null;global.gc?.();
}
await assert.rejects(()=>readParameterBundle(new File([new Uint8Array([0,1,2])],'broken.tar.gz'),manifest.circuits[1]));
console.log('Wrong-question, truncated, and invalid gzip bundles rejected');
