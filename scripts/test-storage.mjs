import assert from 'node:assert/strict';
import fs from 'node:fs/promises';
import os from 'node:os';
import path from 'node:path';
import {pathToFileURL} from 'node:url';
import {createHash} from 'node:crypto';
import {manageParameters,parameterParts,readRemoteParameters,withParameterLock} from '../web/parameters.js';
const hash=b=>createHash('sha256').update(b).digest('hex');
const maps=new Map();
const cacheStorage={open:async name=>{
 if(!maps.has(name))maps.set(name,new Map());const map=maps.get(name),key=k=>typeof k==='string'?k:k.url;
 return {keys:async()=>[...map.keys()].map(url=>({url})),match:async k=>map.get(key(k))?.clone(),put:async(k,r)=>map.set(key(k),r.clone()),delete:async k=>map.delete(key(k))};
}};
const infos=[1,2,3].map(q=>{
 const bytes=Buffer.from(`parameter ${q}`);
 return {pk_bytes:bytes.length,pk_sha256:hash(bytes),chunks:[{file:`q${q}-0.dat`,bytes:bytes.length,sha256:hash(bytes)}]};
});
const opts={cacheStorage,infos:Object.fromEntries(infos.map((info,index)=>[index+1,info]))};
const cache=await cacheStorage.open('prp-proving-components-v1');
for(let q=1;q<=3;q++)await cache.put(parameterParts(q,infos[q-1])[0].key,new Response(`parameter ${q}`));
const legacy='https://raw.githubusercontent.com/owner/repo/old-commit/public/groth16/q1-0.dat';
await cache.put(legacy,new Response('parameter 1'));
const unrelated=await cacheStorage.open('unrelated');await unrelated.put('https://example.org/certificate',new Response('keep'));
let state=await manageParameters('list',null,opts);
assert.deepEqual(state.rows.map(r=>r.bytes),[22,11,11]);
assert.deepEqual(state.rows.map(r=>r.cached_bytes),[11,11,11]);
assert.deepEqual(state.rows.map(r=>r.total_bytes),[11,11,11]);
assert.equal((await cache.keys()).length,4,'inspection must not delete existing data');
await readRemoteParameters(1,infos[0],()=>{},{...opts,fetch:()=>{throw Error('unexpected download');}});
assert.equal((await cache.keys()).length,3,'same-content URL duplicates are removed on reuse');
state=await manageParameters('delete',2,opts);assert.deepEqual(state.rows.map(r=>r.bytes>0),[true,false,true]);
state=await manageParameters('clear',null,opts);assert.ok(state.rows.every(r=>r.bytes===0));assert.ok(await unrelated.match('https://example.org/certificate'));
// Existing deployed cache entries migrate without downloading; malformed entries cannot bypass authentication.
await cache.put(legacy,new Response('parameter 1'));
assert.equal(Buffer.from(await readRemoteParameters(1,infos[0],()=>{},{...opts,fetch:()=>{throw Error('unexpected download during migration');}})).toString(),'parameter 1');
assert.ok(await cache.match(parameterParts(1,infos[0])[0].key));assert.equal(await cache.match(legacy),undefined);
await manageParameters('clear',null,opts);
await cache.put(legacy,new Response('bad content'));
let downloads=0;
await readRemoteParameters(1,infos[0],()=>{},{...opts,fetch:async()=>{downloads++;return new Response('parameter 1');}});
assert.equal(downloads,1);
assert.equal(await cache.match(legacy),undefined);
// Deletion removes every Q1 entry; the next load downloads and becomes visible again.
state=await manageParameters('delete',1,opts);assert.equal(state.rows[0].bytes,0);
downloads=0;
await readRemoteParameters(1,infos[0],()=>{},{...opts,fetch:async()=>{downloads++;return new Response('parameter 1');}});
assert.equal(downloads,1);
assert.equal((await manageParameters('list',null,opts)).rows[0].bytes,infos[0].pk_bytes);
assert.equal((await manageParameters('list',null,{...opts,cacheStorage:{open:async()=>{throw Error('disabled');}}})).available,false);
await assert.rejects(()=>manageParameters('clear',null,{...opts,cacheStorage:{open:async()=>null}}),/无法访问/);
await assert.rejects(()=>manageParameters('delete',4,opts),/无效/);
const unavailable={request:async(_n,_o,fn)=>fn(null)};
await assert.rejects(async()=>withParameterLock(()=>{throw Error('must not enter');},true,unavailable),/正在使用/);
await assert.rejects(()=>manageParameters('clear',null,{...opts,locks:unavailable}),/正在使用/);
assert.ok((await manageParameters('list',null,{...opts,locks:unavailable})).available,'inspection remains available during generation');
// A failed chunk must not release the lock while another loader still writes.
let released=false,finishChunk,startedChunk;
const started=new Promise(resolve=>{startedChunk=resolve;});
const delayed=new Promise(resolve=>{finishChunk=resolve;});
const pair={...infos[0],pk_bytes:22,chunks:[infos[0].chunks[0],{...infos[0].chunks[0],file:'q1-1.dat'}]};
const failing=readRemoteParameters(1,pair,()=>{},{
 cacheStorage:{open:async()=>null},
 locks:{request:async(_name,options,fn)=>{assert.equal(options.mode,'shared');try{return await fn({});}finally{released=true;}}},
 fetch:async url=>{
  if(url.endsWith('q1-0.dat'))return new Response('',{status:401});
  startedChunk();await delayed;return new Response('parameter 1');
 }
}).then(()=>assert.fail('expected a download failure'),error=>error);
await started;assert.equal(released,false);finishChunk();
assert.match((await failing).message,/刷新/);assert.equal(released,true);
// Deployment pins change the download URL, never the cache key.
const dir=await fs.mkdtemp(path.join(os.tmpdir(),'prp-cache-test-'));
try{
 const source=await fs.readFile(new URL('../web/parameters.js',import.meta.url),'utf8');
 const modules=[];
 for(const ref of ['commit-a','commit-b']){
  const file=path.join(dir,`${ref}.mjs`);await fs.writeFile(file,source.replace("const PARAMETER_BASE_URL = '';",`const PARAMETER_BASE_URL = 'https://raw.githubusercontent.com/owner/repo/${ref}/public/groth16/';`));
  modules.push(await import(pathToFileURL(file)));
 }
 const a=modules[0].parameterParts(1,infos[0])[0],b=modules[1].parameterParts(1,infos[0])[0];
 assert.notEqual(a.url,b.url);assert.equal(a.key,b.key);
 await manageParameters('clear',null,opts);downloads=0;
 await modules[0].readRemoteParameters(1,infos[0],()=>{},{...opts,fetch:async()=>{downloads++;return new Response('parameter 1');}});
 await modules[1].readRemoteParameters(1,infos[0],()=>{},{...opts,fetch:()=>{throw Error('unexpected download after deployment');}});
 assert.equal(downloads,1);
}finally{await fs.rm(dir,{recursive:true,force:true});}
// Incomplete and wrong-sized chunks do not imply a complete parameter package.
await manageParameters('clear',null,opts);
const partialInfo={pk_bytes:22,pk_sha256:hash(Buffer.from('parameter 1parameter 1')),chunks:[infos[0].chunks[0],{...infos[0].chunks[0],file:'q1-1.dat'}]};
const partialOpts={...opts,infos:{...opts.infos,1:partialInfo}};
const partialParts=parameterParts(1,partialInfo);
await cache.put(partialParts[0].key,new Response('parameter 1'));
state=await manageParameters('list',null,partialOpts);
assert.equal(state.rows[0].cached_bytes,11);assert.equal(state.rows[0].total_bytes,22);
await cache.put(partialParts[1].key,new Response('truncated'));
state=await manageParameters('list',null,partialOpts);assert.equal(state.rows[0].cached_bytes,11);
await cache.put(partialParts[1].key,new Response('parameter 1'));
state=await manageParameters('list',null,partialOpts);assert.equal(state.rows[0].cached_bytes,22);
console.log('Storage: inspection, selective/all deletion, legacy migration, cross-deployment reuse, integrity and locking pass');
