import fs from 'node:fs/promises';
import {createRequire} from 'node:module';
const require=createRequire(import.meta.url);
const wasm=require(process.env.CORE_WASM_MODULE||'../.cache/release-core-test/flight_core.js');
const q=Number(process.argv[2]||1),action=process.argv[3]||'prove';
const job=JSON.parse(await fs.readFile(`.cache/rust-tests/job-q${q}.json`));job.action=action;
let circuit=new Uint8Array(),g1=new Uint8Array();
if(action==='prove'){
 const info=JSON.parse(await fs.readFile('public/groth16/manifest.json')).circuits[q];
 g1=new Uint8Array(info.pk_bytes);let offset=0;
 for(const chunk of info.chunks){const bytes=await fs.readFile(`public/groth16/${chunk.file}`);g1.set(bytes,offset);offset+=bytes.length;}
}
const started=performance.now();
const result=JSON.parse(wasm.execute(JSON.stringify(job),circuit,g1,text=>console.log(text)));
console.log(JSON.stringify({q,action,seconds:(performance.now()-started)/1000,maxRSS:process.resourceUsage().maxRSS}));
await fs.writeFile(`.cache/groth16/wasm-q${q}-${action}.json`,JSON.stringify(result));
if(action==='prove'){
 const again=JSON.parse(wasm.execute(JSON.stringify({action:'verify',certificate:result.certificate}),new Uint8Array(),new Uint8Array(),()=>{}));
 if(JSON.stringify(again.scores)!==JSON.stringify(result.scores))throw Error('roundtrip mismatch');
 const changed=structuredClone(result.certificate);changed.metadata.team='篡改队名';
 let rejected=false;try{wasm.execute(JSON.stringify({action:'verify',certificate:changed}),new Uint8Array(),new Uint8Array(),()=>{});}catch{rejected=true;}
 if(!rejected)throw Error('tampered metadata accepted');
 console.log('WASM proof, roundtrip and tamper checks passed');
}
