// Rebuild all browser engines from source. Pages and Release CI run this first.
import fs from 'node:fs/promises';
import {spawnSync} from 'node:child_process';
import {sourceHashes,artifactHashes} from './browser-release.mjs';
function run(command,args,env=process.env){const r=spawnSync(command,args,{stdio:'inherit',shell:false,env});if(r.error)throw r.error;if(r.status)process.exit(r.status);}
run(process.env.CARGO||'cargo',['build','--locked','--workspace','--target','wasm32-unknown-unknown','--release','--lib']);
const target=process.env.CARGO_TARGET_DIR||'target';
const threadedTarget=`${target}/web-threads`;
run('rustup',['run',process.env.WASM_THREADS_TOOLCHAIN||'nightly-2025-11-15','cargo','build','--locked','-p','flight-core','--features','web-threads','--target','wasm32-unknown-unknown','--release','--lib','-Z','build-std=std,panic_abort'],{
 ...process.env,CARGO_TARGET_DIR:threadedTarget,
 RUSTFLAGS:'-C target-feature=+atomics,+bulk-memory,+mutable-globals -C link-arg=--shared-memory -C link-arg=--import-memory -C link-arg=--export=__wasm_init_tls -C link-arg=--export=__tls_base -C link-arg=--export=__tls_size -C link-arg=--export=__tls_align -C link-arg=--max-memory=4294967296'
});
await fs.mkdir('web-dist',{recursive:true});
for(const name of await fs.readdir('web-dist'))await fs.rm(`web-dist/${name}`,{recursive:true,force:true});
for(const [crate,dir,build] of [['flight_ui','ui',target],['flight_core','engine',target],['flight_core','engine-threaded',threadedTarget]])run(process.env.WASM_BINDGEN||'wasm-bindgen',['--target','web','--out-dir',`web-dist/${dir}`,'--out-name',crate,`${build}/wasm32-unknown-unknown/release/${crate}.wasm`]);
// Keep the pinned Rayon ABI but add startup failure handling and page-owned workers.
const snippets='web-dist/engine-threaded/snippets';
const rayon=(await fs.readdir(snippets)).filter(name=>name.startsWith('wasm-bindgen-rayon-'));
if(rayon.length!==1)throw Error('Unexpected wasm-bindgen-rayon output');
await fs.copyFile('web/rayon-worker.js',`${snippets}/${rayon[0]}/src/workerHelpers.no-bundler.js`);
for(const name of await fs.readdir('web'))if(name.endsWith('.js'))await fs.copyFile(`web/${name}`,`web-dist/${name}`);
await fs.writeFile('web-dist/release.json',JSON.stringify({format:1,sources:await sourceHashes(),artifacts:await artifactHashes()},null,2));
await import('./build.mjs');
