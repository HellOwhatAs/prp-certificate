// Public proving components only. CSV answers and certificates are never cached here.
const CACHE_NAME='prp-proving-components-v1';
// The Pages build pins this URL to the deployed repository commit.
const PARAMETER_BASE_URL = '';
const digest=async bytes=>Array.from(new Uint8Array(await crypto.subtle.digest('SHA-256',bytes)),x=>x.toString(16).padStart(2,'0')).join('');
const mb=bytes=>(bytes/1_000_000).toFixed(0);
export function parameterParts(q,info){
 if(![1,2,3].includes(q)||!Number.isSafeInteger(info?.pk_bytes)||info.pk_bytes<1||info.pk_bytes>600_000_000||!(/^[a-f0-9]{64}$/).test(info.pk_sha256)||!Array.isArray(info.chunks)||info.chunks.length>200)throw Error('证明组件清单无效');
 let offset=0;const names=new Set();
 const parts=info.chunks.map(part=>{
  if(!new RegExp(`^q${q}-[0-9]+\\.dat$`).test(part.file)||names.has(part.file)||!Number.isSafeInteger(part.bytes)||part.bytes<1||part.bytes>4_194_304||!(/^[a-f0-9]{64}$/).test(part.sha256))throw Error('证明组件清单无效');
  names.add(part.file);const next={...part,offset,key:new URL(`./parameter-cache/${info.pk_sha256}/${part.file}`,import.meta.url).href,url:PARAMETER_BASE_URL?new URL(part.file,PARAMETER_BASE_URL).href:new URL(`./parameters/${info.pk_sha256}/${part.file}`,import.meta.url).href};offset+=part.bytes;return next;
 });
 if(offset!==info.pk_bytes)throw Error('证明组件长度无效');
 return parts;
}
async function boundedBytes(response,length){
 const claimed=response.headers.get('content-length');
 if(claimed!==null&&Number(claimed)!==length)throw Error('证明组件长度无效');
 const out=new Uint8Array(length);let offset=0;
 const reader=response.body?.getReader();if(!reader)throw Error('证明组件为空');
 try{for(;;){const {done,value}=await reader.read();if(done)break;if(offset+value.length>length)throw Error('证明组件过大');out.set(value,offset);offset+=value.length;}}
 finally{await reader.cancel().catch(()=>{});reader.releaseLock();}
 if(offset!==length)throw Error('证明组件下载不完整');return out;
}
async function readParameters(q,info,progress=()=>{},options={}){
 const parts=parameterParts(q,info),out=new Uint8Array(info.pk_bytes);
 const fetcher=options.fetch??globalThis.fetch;
 const storage=options.cacheStorage??globalThis.caches;
 let cache=null;try{cache=await storage?.open(CACHE_NAME);}catch{}
 let legacy=[];
 if(cache){try{legacy=await cache.keys();}catch{}}
 let next=0,loaded=0,downloaded=0;
 progress(`准备证明组件（首次约 ${mb(info.pk_bytes)} MB）；正在检查缓存…`);
 async function load(){
  for(;;){const index=next++;if(index>=parts.length)return;const part=parts[index];let bytes;
   if(cache){
    // The network location can change at every deployment; cache identity cannot.
    // Reuse matching old URL entries only after checking their content hash.
    const aliases=legacy.filter(r=>r.url!==part.key&&r.url.endsWith('/'+part.file));
    for(const key of [part.key,...aliases.map(r=>r.url)]){
     try{
      const saved=await cache.match(key);if(!saved)continue;
      const candidate=await boundedBytes(saved,part.bytes);
      if(await digest(candidate)!==part.sha256){if(key===part.key)await cache.delete(key);continue;}
      bytes=candidate;
      if(key!==part.key)await cache.put(part.key,parameterResponse(bytes));
      // Once the current chunk is stored, obsolete copies of this chunk are disposable.
      for(const alias of aliases)await cache.delete(alias);
      break;
     }catch{if(bytes)break;try{if(key===part.key)await cache.delete(key);}catch{}}
    }
   }
   if(!bytes){
    for(let attempt=0;attempt<3;attempt++){
     try{
      const response=await fetcher(part.url,{cache:'no-store',credentials:'same-origin',signal:AbortSignal.timeout(45_000)});
      if(!response.ok){const e=Error(response.status===401||response.status===403?'资源访问已过期，请刷新页面后重试':'证明组件下载失败，请检查网络后重试');e.permanent=response.status<500&&response.status!==429;throw e;}
      const candidate=await boundedBytes(response,part.bytes);if(await digest(candidate)!==part.sha256)throw Error('证明组件校验失败，请重试');bytes=candidate;downloaded+=bytes.length;break;
     }catch(error){if(error.permanent||attempt===2)throw Error(error.message==='Failed to fetch'?'证明组件下载失败，已下载部分会保留，请重试':error.message);await new Promise(resolve=>setTimeout(resolve,250*(attempt+1)));}
    }
    if(cache){try{await cache.put(part.key,parameterResponse(bytes));for(const alias of legacy.filter(r=>r.url!==part.key&&r.url.endsWith('/'+part.file)))await cache.delete(alias);}catch{cache=null;}}
   }
   out.set(bytes,part.offset);loaded+=bytes.length;
   progress(`${downloaded?'准备':'读取已缓存的'}证明组件 ${mb(loaded)} / ${mb(info.pk_bytes)} MB…`);
  }
 }
 // Keep the shared lock until every in-flight cache write has finished, even on failure.
 const results=await Promise.allSettled(Array.from({length:Math.min(3,parts.length)},load));
 const failure=results.find(result=>result.status==='rejected');if(failure)throw failure.reason;
 return out;
}

function parameterResponse(bytes){
 return new Response(bytes,{headers:{'content-type':'application/octet-stream','content-length':String(bytes.length)}});
}
// Readers coexist; a deletion cannot overlap another tab's parameter loading.
export function withParameterLock(task,exclusive=false,locks=globalThis.navigator?.locks){
 return locks?locks.request('prp-parameter-cache',{mode:exclusive?'exclusive':'shared',ifAvailable:true},lock=>{
  if(!lock)throw Error('参数缓存正在使用，请稍后重试');return task();
 }):task();
}
export function readRemoteParameters(q,info,progress=()=>{},options={}){
 return withParameterLock(()=>readParameters(q,info,progress,options),false,options.locks);
}
let storageManifest;
async function storageInfos(){
 if(!storageManifest)storageManifest=(async()=>{
  const response=await fetch(new URL('./groth16/manifest.json',import.meta.url));
  if(!response.ok)throw Error('证明组件清单加载失败');
  return (await response.json()).circuits;
 })().catch(error=>{storageManifest=null;throw error;});
 return storageManifest;
}
export async function manageParameters(operation='list',q=null,options={}){
 if(!['list','delete','clear'].includes(operation)||(operation==='delete'&&![1,2,3].includes(q)))throw Error('存储操作无效');
 const task=async()=>{
  const infos=options.infos??await storageInfos();
  const parts=[1,2,3].map(q=>parameterParts(q,infos[q]));
  const rows=[1,2,3].map(q=>({q,bytes:0,cached_bytes:0,total_bytes:infos[q].pk_bytes}));
  const counted=new Set();
  const storage=options.cacheStorage??globalThis.caches;
  let cache;try{cache=await storage?.open(CACHE_NAME);}catch{}
  if(!cache){
   if(operation!=='list')throw Error('无法访问参数缓存');
   return {available:false,rows};
  }
  let other_bytes=0;
  for(const request of await cache.keys()){
   const question=Number(new URL(request.url).pathname.match(/\/q([123])-\d+\.dat$/)?.[1]);
   if(operation==='clear'||(operation==='delete'&&question===q)){
    await cache.delete(request);continue;
   }
   const response=await cache.match(request);if(!response)continue;
   const length=response.headers.get('content-length');
   const size=length!==null&&/^\d+$/.test(length)&&Number.isSafeInteger(Number(length))?Number(length):(await response.clone().arrayBuffer()).byteLength;
   if(question){
    const row=rows[question-1];row.bytes+=size;
    const part=parts[question-1].find(part=>request.url.endsWith('/'+part.file));
    if(part&&size===part.bytes&&!counted.has(part.key)){
     // Stable keys contain the parameter hash. Old URL keys need authentication
     // before they can count towards this version's download progress.
     let current=request.url===part.key;
     if(!current){try{current=await digest(await boundedBytes(response,part.bytes))===part.sha256;}catch{}}
     if(current){counted.add(part.key);row.cached_bytes+=part.bytes;}
    }
   }else other_bytes+=size;
  }
  return {available:true,rows,other_bytes};
 };
 return operation==='list'?task():withParameterLock(task,true,options.locks);
}
