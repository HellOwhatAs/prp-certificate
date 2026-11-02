import fs from 'node:fs';
import {spawnSync} from 'node:child_process';
import {createRequire} from 'node:module';
import assert from 'node:assert/strict';
function run(cmd,args){const r=spawnSync(cmd,args,{stdio:'inherit'});if(r.error)throw r.error;if(r.status)process.exit(r.status);}
run(process.env.CARGO||'cargo',['run','--release','-p','flight-ui','--locked','--example','certificate_vectors','--','.cache/carrier-vectors']);
run(process.env.CARGO||'cargo',['build','-p','flight-ui','--locked','--target','wasm32-unknown-unknown','--release','--lib']);
run(process.env.WASM_BINDGEN||'wasm-bindgen',['--target','nodejs','--out-dir','.cache/carrier-codec','--out-name','flight_ui','target/wasm32-unknown-unknown/release/flight_ui.wasm']);
fs.writeFileSync('.cache/carrier-codec/package.json','{"type":"commonjs"}');
const require=createRequire(import.meta.url),codec=require('../.cache/carrier-codec/flight_ui.js');
for(let q=1;q<=3;q++){
 const item=fs.readFileSync(`.cache/carrier-vectors/q${q}.json`,'utf8');
 const png=fs.readFileSync(`.cache/carrier-vectors/q${q}.png`);
 assert.deepEqual(Buffer.from(codec.certificate_png(item)),png);
 assert.deepEqual(JSON.parse(codec.parse_certificate_file(png)),JSON.parse(item).certificate);
 console.log(`Q${q}: Groth16 QR PNG native/WASM byte-identical; complete payload recovered`);
}
assert.throws(()=>codec.parse_certificate_file(new TextEncoder().encode('%PDF-1.7')));
assert.equal(codec.certificate_pdf,undefined);assert.equal(codec.check_certificate_pdf,undefined);
