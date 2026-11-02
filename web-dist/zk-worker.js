import {readParameterBundle} from './parameter-bundle.js';
import {readRemoteParameters} from './parameters.js';
import init,{execute} from './engine/flight_core.js';
const send=(type,data)=>postMessage({type,...data});
onmessage=async ({data})=>{
 try {
  send('progress',{text:'加载 Rust 计算引擎…'});
  await init();
  let circuit=new Uint8Array(),g1=new Uint8Array();
  if(data.action==='import'){
   send('progress',{text:'读取证书与证明…'});
   if(typeof data.file!=='string'||data.file.length>22_000_000)throw Error('证书文件过大');
   const bytes=Uint8Array.from(atob(data.file),c=>c.charCodeAt(0));
   const codec=await import('./ui/flight_ui.js');await codec.default();
   const certificate=JSON.parse(codec.parse_certificate_file(bytes));
   const verified=execute(JSON.stringify({action:'verify',certificate}),circuit,g1,text=>send('progress',{text}));
   send('result',{result:JSON.parse(verified)});return;
  }
  const read=async path=>{
   const response=await fetch(new URL(path,import.meta.url));
   if(!response.ok)throw Error(`公开参数加载失败（${response.status}），请检查网络后重试`);
   return new Uint8Array(await response.arrayBuffer());
  };
  if(data.action==='prove'){
   if(![1,2,3].includes(data.q))throw Error('题号无效');
   // Reject invalid CSV before downloading or reading large proving components.
   const checked=JSON.parse(execute(JSON.stringify({action:'check',q:data.q,files:data.files}),circuit,g1,text=>send('progress',{text})));
   if(!checked.canProve)throw Error('当前证明上限为 256 架次');
   const manifest=JSON.parse(new TextDecoder().decode(await read('./groth16/manifest.json')));
   const info=manifest.circuits[data.q];
   if(!info||!Number.isSafeInteger(info.pk_bytes)||info.pk_bytes>600_000_000)throw Error('公开参数清单无效');
   const file=data.parameterFile;delete data.parameterFile;
   g1=file?await readParameterBundle(file,info,text=>send('progress',{text})):await readRemoteParameters(data.q,info,text=>send('progress',{text}));
  }
  const result=execute(JSON.stringify(data),circuit,g1,text=>send('progress',{text}));
  send('result',{result:JSON.parse(result)});
 }catch(e){send('error',{message:e instanceof WebAssembly.RuntimeError?'计算失败：证明数据无效或浏览器可用内存不足':String(e?.message||e)});}
};
