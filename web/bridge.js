import {EngineWorker} from './worker-host.js';
import {manageParameters} from './parameters.js';
let worker=null,queue=[];
let pdfWorker=null,pdfSequence=0;
const pdfPending=new Map();
function stopPdfWorker(message='PDF 显示已停止'){
  pdfWorker?.terminate();pdfWorker=null;
  for(const request of pdfPending.values()){
    clearTimeout(request.timer);request.reject(message);
  }
  pdfPending.clear();
}
globalThis.addEventListener?.('pagehide',()=>stopPdfWorker());
globalThis.flightBridge={
  renderProblemPdfPage(page,width){
    return new Promise((resolve,reject)=>{
      try{
        if(!Number.isSafeInteger(page)||page<0||!Number.isSafeInteger(width)||width<1||width>4096)
          throw Error('PDF 显示请求无效');
        if(!pdfWorker){
          const local=new Worker(new URL('./pdf-worker.js',import.meta.url),{type:'module'});
          pdfWorker=local;
          local.onmessage=({data})=>{
            if(pdfWorker!==local)return;
            const request=pdfPending.get(data?.id);
            if(!request)return;
            if(data.error&&data.fatal){stopPdfWorker(data.error);return;}
            pdfPending.delete(data.id);clearTimeout(request.timer);
            if(data.error)request.reject(data.error);
            else if(data.bytes instanceof Uint8Array)request.resolve(data.bytes);
            else request.reject('PDF 显示数据无效');
          };
          local.onerror=event=>{
            event.preventDefault?.();
            if(pdfWorker===local)stopPdfWorker(event.message||'PDF 显示进程异常');
          };
          local.onmessageerror=()=>{
            if(pdfWorker===local)stopPdfWorker('PDF 显示数据读取失败');
          };
        }
        const id=++pdfSequence;
        const timer=setTimeout(()=>stopPdfWorker('PDF 显示超时'),60_000);
        pdfPending.set(id,{resolve,reject,timer});
        try{pdfWorker.postMessage({id,page,width});}
        catch(error){stopPdfWorker(error.message||String(error));}
      }catch(error){reject(error.message||String(error));}
    });
  },
  async manageStorage(operation,q){
    if(worker&&operation!=='list')throw '请在当前任务结束后清理缓存';
    try{return JSON.stringify(await manageParameters(operation,q||null));}
    catch(error){throw error.message||String(error);}
  },
  chooseFiles(slot){
    return new Promise((resolve,reject)=>{
      const input=document.createElement('input');
      input.type='file';input.accept=slot===6?'.png,.jpg,.jpeg':'.csv';input.multiple=slot===6;
      input.hidden=true;document.body.append(input);
      const finish=(value,error)=>{input.remove();error?reject(error.message||String(error)):resolve(JSON.stringify(value));};
      input.addEventListener('cancel',()=>finish([]),{once:true});
      input.addEventListener('change',async()=>{
        try{
          const files=Array.from(input.files||[]);
          if(files.length>20)throw Error('每次最多导入 20 份证书');
          if(files.some(file=>file.size>(slot===6?16_000_000:10_000_000)))throw Error(slot===6?'证书图片不能超过 16 MB':'单个 CSV 不能超过 10 MB');
          const values=[];
          for(const file of files){
            const bytes=new Uint8Array(await file.arrayBuffer());let binary='';
            for(let at=0;at<bytes.length;at+=32768)binary+=String.fromCharCode(...bytes.subarray(at,at+32768));
            values.push({name:file.name,data:btoa(binary)});
          }
          finish(values);
        }catch(error){finish(null,error);}
      },{once:true});
      input.click();
    });
  },
  downloadFile(name,bytes){
    const type=name.endsWith('.png')?'image/png':name.endsWith('.pdf')?'application/pdf':'text/csv;charset=utf-8';
    const url=URL.createObjectURL(new Blob([bytes],{type}));
    const link=document.createElement('a');link.href=url;link.download=name;
    document.body.append(link);link.click();link.remove();setTimeout(()=>URL.revokeObjectURL(url),1000);
  },
  send(text){
    if(worker)throw Error('已有任务运行中');
    const data=JSON.parse(text);
    const local=new EngineWorker();worker=local;
    const done=()=>{local.terminate();if(worker===local)worker=null;};
    local.onmessage=e=>{if(worker!==local)return;queue.push(e.data);if(e.data.type!=='progress')done();};
    local.onerror=e=>{if(worker!==local)return;queue.push({type:'error',message:e.message||'计算进程异常，请检查可用内存'});done();};
    local.postMessage(data);
  },
  drain(){const messages=queue;queue=[];return JSON.stringify(messages);},
  cancel(){worker?.terminate();worker=null;queue=[];}
};
