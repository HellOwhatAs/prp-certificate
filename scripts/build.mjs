// Build the static site without requiring a Rust toolchain on the hosting service.
// Reject stale sources or altered precompiled WASM; rebuild with npm run build:rust.
import fs from 'node:fs/promises';
import {sourceHashes,artifactHashes} from './browser-release.mjs';
const release=JSON.parse(await fs.readFile('web-dist/release.json','utf8'));
if(release.format!==1||JSON.stringify(release.sources)!==JSON.stringify(await sourceHashes())||JSON.stringify(release.artifacts)!==JSON.stringify(await artifactHashes()))throw Error('网页编译文件与源码不匹配，请先运行 npm run build:rust');
await fs.rm('dist',{recursive:true,force:true});
await fs.mkdir('dist/client',{recursive:true});
await fs.cp('public','dist/client',{recursive:true,filter:path=>!path.endsWith('.dat')&&!path.split('/').at(-1).startsWith('.')});
for(const name of ['ui','engine','bridge.js','zk-worker.js','parameter-bundle.js','parameters.js'])await fs.cp(`web-dist/${name}`,`dist/client/${name}`,{recursive:true});
await fs.writeFile('dist/client/.nojekyll','');
