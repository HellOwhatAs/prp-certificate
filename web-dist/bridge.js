let worker=null,queue=[];
globalThis.flightBridge={
  send(text){
    if(worker)throw Error('已有任务运行中');
    const data=JSON.parse(text);
    const local=new Worker(new URL('./zk-worker.js',import.meta.url),{type:'module'});worker=local;
    const done=()=>{local.terminate();if(worker===local)worker=null;};
    local.onmessage=e=>{queue.push(e.data);if(e.data.type!=='progress')done();};
    local.onerror=e=>{queue.push({type:'error',message:e.message||'计算进程异常，请检查可用内存'});done();};
    local.postMessage(data);
  },
  drain(){const messages=queue;queue=[];return JSON.stringify(messages);},
  cancel(){worker?.terminate();worker=null;queue=[];}
};
