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
  names.add(part.file);const next={...part,offset,url:PARAMETER_BASE_URL?new URL(part.file,PARAMETER_BASE_URL).href:new URL(`./parameters/${info.pk_sha256}/${part.file}`,import.meta.url).href};offset+=part.bytes;return next;
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
export async function readRemoteParameters(q,info,progress=()=>{},options={}){
 const parts=parameterParts(q,info),out=new Uint8Array(info.pk_bytes);
 const fetcher=options.fetch??globalThis.fetch;
 const storage=options.cacheStorage??globalThis.caches;
 let cache=null;try{cache=await storage?.open(CACHE_NAME);}catch{}
 let next=0,loaded=0,downloaded=0;
 progress(`准备证明组件（首次约 ${mb(info.pk_bytes)} MB）；正在检查缓存…`);
 async function load(){
  for(;;){const index=next++;if(index>=parts.length)return;const part=parts[index];let bytes;
   if(cache){try{const saved=await cache.match(part.url);if(saved){const candidate=await boundedBytes(saved,part.bytes);if(await digest(candidate)===part.sha256)bytes=candidate;else await cache.delete(part.url);}}catch{try{await cache.delete(part.url);}catch{}}}
   if(!bytes){
    for(let attempt=0;attempt<3;attempt++){
     try{
      const response=await fetcher(part.url,{cache:'no-store',credentials:'same-origin',signal:AbortSignal.timeout(45_000)});
      if(!response.ok){const e=Error(response.status===401||response.status===403?'资源访问已过期，请刷新页面后重试':'证明组件下载失败，请检查网络后重试');e.permanent=response.status<500&&response.status!==429;throw e;}
      const candidate=await boundedBytes(response,part.bytes);if(await digest(candidate)!==part.sha256)throw Error('证明组件校验失败，请重试');bytes=candidate;downloaded+=bytes.length;break;
     }catch(error){if(error.permanent||attempt===2)throw Error(error.message==='Failed to fetch'?'证明组件下载失败，已下载部分会保留，请重试':error.message);await new Promise(resolve=>setTimeout(resolve,250*(attempt+1)));}
    }
    if(cache){try{await cache.put(part.url,new Response(bytes,{headers:{'content-type':'application/octet-stream','content-length':String(bytes.length)}}));}catch{cache=null;}}
   }
   out.set(bytes,part.offset);loaded+=bytes.length;
   progress(`${downloaded?'准备':'读取已缓存的'}证明组件 ${mb(loaded)} / ${mb(info.pk_bytes)} MB…`);
  }
 }
 await Promise.all(Array.from({length:Math.min(3,parts.length)},load));
 return out;
}
