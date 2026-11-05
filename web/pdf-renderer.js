// Reuse the PDF renderer across pages, independently of cancellable proof jobs.
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

export function renderProblemPdfPage(page,width){
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
}
