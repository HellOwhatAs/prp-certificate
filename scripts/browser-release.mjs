import fs from 'node:fs/promises';
import {createHash} from 'node:crypto';
export const hash=bytes=>createHash('sha256').update(bytes).digest('hex');
async function walk(path){const files=[];for(const item of await fs.readdir(path,{withFileTypes:true})){const next=`${path}/${item.name}`;if(item.isDirectory())files.push(...await walk(next));else if(item.isFile()&&!next.endsWith('.dat'))files.push(next);}return files;}
export async function sourceHashes(){
 const paths=['Cargo.toml','Cargo.lock','package.json','package-lock.json','scripts/build.mjs','scripts/build-rust.mjs','scripts/browser-release.mjs'];
 for(const folder of ['crates','assets','web','public'])paths.push(...await walk(folder));
 return Promise.all(paths.sort().map(async path=>({path,sha256:hash(await fs.readFile(path))})));
}
export async function artifactHashes(){return Promise.all((await walk('web-dist')).filter(path=>!path.endsWith('release.json')).sort().map(async path=>({path,sha256:hash(await fs.readFile(path))})));}
