// Assemble a previously compiled local build; CI first runs npm run build:rust.
// Reject missing/stale sources or altered WASM instead of publishing old code.
import fs from 'node:fs/promises';
import {sourceHashes,artifactHashes} from './browser-release.mjs';
let release;
try { release=JSON.parse(await fs.readFile('web-dist/release.json','utf8')); }
catch(error) { if(error.code==='ENOENT')throw Error('网页编译文件缺失，请先运行 npm run build:rust');throw error; }
if(release.format!==1||JSON.stringify(release.sources)!==JSON.stringify(await sourceHashes())||JSON.stringify(release.artifacts)!==JSON.stringify(await artifactHashes()))throw Error('网页编译文件与源码不匹配，请先运行 npm run build:rust');
await fs.rm('dist',{recursive:true,force:true});
await fs.mkdir('dist/client',{recursive:true});
await fs.cp('public','dist/client',{recursive:true,filter:path=>!path.endsWith('.dat')&&!path.split('/').at(-1).startsWith('.')});
for(const name of (await fs.readdir('web-dist')).filter(name=>name!=='release.json'))await fs.cp(`web-dist/${name}`,`dist/client/${name}`,{recursive:true});
await fs.writeFile('dist/client/.nojekyll','');
