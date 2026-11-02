// Explicit Rust rebuild. Normal hosting builds validate and copy these checked-in assets.
import fs from 'node:fs/promises';
import {spawnSync} from 'node:child_process';
import {sourceHashes,artifactHashes} from './browser-release.mjs';
function run(command,args){const r=spawnSync(command,args,{stdio:'inherit',shell:false});if(r.error)throw r.error;if(r.status)process.exit(r.status);}
run(process.env.CARGO||'cargo',['build','--locked','--workspace','--target','wasm32-unknown-unknown','--release','--lib']);
await fs.mkdir('web-dist',{recursive:true});
for(const name of await fs.readdir('web-dist'))await fs.rm(`web-dist/${name}`,{recursive:true,force:true});
for(const [crate,dir] of [['flight_ui','ui'],['flight_core','engine']])run(process.env.WASM_BINDGEN||'wasm-bindgen',['--target','web','--out-dir',`web-dist/${dir}`,'--out-name',crate,`target/wasm32-unknown-unknown/release/${crate}.wasm`]);
await fs.copyFile('web/bridge.js','web-dist/bridge.js');await fs.copyFile('web/zk-worker.js','web-dist/zk-worker.js');await fs.copyFile('web/parameter-bundle.js','web-dist/parameter-bundle.js');
await fs.copyFile('web/parameters.js','web-dist/parameters.js');
await fs.writeFile('web-dist/release.json',JSON.stringify({format:1,sources:await sourceHashes(),artifacts:await artifactHashes()},null,2));
await import('./build.mjs');
