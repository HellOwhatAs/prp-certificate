import assert from 'node:assert/strict';
import fs from 'node:fs/promises';
import {pathToFileURL} from 'node:url';
import path from 'node:path';

const workers=[];
let pagehide;
globalThis.addEventListener=(type,listener)=>{if(type==='pagehide')pagehide=listener;};
class WorkerStub {
  constructor(url,options){this.url=url;this.options=options;this.sent=[];workers.push(this);}
  postMessage(data){if(this.failSend)throw Error('post failed');this.sent.push(data);}
  terminate(){this.stopped=true;}
}
globalThis.Worker=WorkerStub;
await import('../web/bridge.js');
const bridge=globalThis.flightBridge;
const rejected=(promise,pattern)=>assert.rejects(promise,error=>pattern.test(String(error)));
const answer=(worker,bytes=new Uint8Array([1,2,3]),index=-1)=>{
  const request=worker.sent.at(index);
  worker.onmessage({data:{id:request.id,bytes}});
  return bytes;
};

const first=bridge.renderProblemPdfPage(0,612);
assert.equal(workers.length,1);
assert.equal(workers[0].url.pathname.endsWith('/web/pdf-worker.js'),true);
assert.deepEqual(workers[0].options,{type:'module'});
answer(workers[0]);
assert.deepEqual(await first,new Uint8Array([1,2,3]));

// The same worker handles subsequent pages; request IDs also reject stale replies.
const second=bridge.renderProblemPdfPage(1,900);
const third=bridge.renderProblemPdfPage(8,612);
assert.equal(workers.length,1);
answer(workers[0],new Uint8Array([8]),-1);
answer(workers[0],new Uint8Array([2]),-2);
assert.deepEqual(await second,new Uint8Array([2]));
assert.deepEqual(await third,new Uint8Array([8]));

const pageError=rejected(bridge.renderProblemPdfPage(9,612),/页码/);
workers[0].onmessage({data:{id:workers[0].sent.at(-1).id,error:'PDF 页码无效'}});
await pageError;
assert.equal(workers[0].stopped,undefined);

const crashA=rejected(bridge.renderProblemPdfPage(0,612),/worker failed/);
const crashB=rejected(bridge.renderProblemPdfPage(1,612),/worker failed/);
workers[0].onerror({message:'worker failed',preventDefault(){}});
await Promise.all([crashA,crashB]);
assert.equal(workers[0].stopped,true);
const recovered=bridge.renderProblemPdfPage(0,612);
assert.equal(workers.length,2);
// A late message from the terminated instance must not resolve the new request.
workers[0].onmessage({data:{id:workers[1].sent.at(-1).id,bytes:new Uint8Array([99])}});
answer(workers[1],new Uint8Array([4]));
assert.deepEqual(await recovered,new Uint8Array([4]));

const malformed=rejected(bridge.renderProblemPdfPage(0,612),/数据无效/);
workers[1].onmessage({data:{id:workers[1].sent.at(-1).id,bytes:'not a bitmap'}});
await malformed;
const messageError=rejected(bridge.renderProblemPdfPage(0,612),/数据读取失败/);
workers[1].onmessageerror();
await messageError;
assert.equal(workers[1].stopped,true);

globalThis.Worker=class {constructor(){throw Error('worker unavailable');}};
await rejected(bridge.renderProblemPdfPage(0,612),/worker unavailable/);
globalThis.Worker=WorkerStub;
const readyForSendFailure=bridge.renderProblemPdfPage(0,612);
answer(workers[2]);await readyForSendFailure;
workers[2].failSend=true;
await rejected(bridge.renderProblemPdfPage(0,612),/post failed/);
assert.equal(workers[2].stopped,true);

const hidden=rejected(bridge.renderProblemPdfPage(0,612),/显示已停止/);
pagehide();await hidden;
assert.equal(workers[3].stopped,true);
const count=workers.length;
await rejected(bridge.renderProblemPdfPage(0,0),/请求无效/);
await rejected(bridge.renderProblemPdfPage(-1,612),/请求无效/);
assert.equal(workers.length,count);
const loadFailed=rejected(bridge.renderProblemPdfPage(0,612),/module unavailable/);
const failedImport=workers.at(-1);
failedImport.onmessage({data:{id:failedImport.sent.at(-1).id,error:'module unavailable',fatal:true}});
await loadFailed;
assert.equal(failedImport.stopped,true);
const importedAgain=bridge.renderProblemPdfPage(0,612);
assert.notEqual(workers.at(-1),failedImport);
answer(workers.at(-1));await importedAgain;
pagehide();

// Saving a material keeps its exact bytes/name and uses the original file type.
const blobs=[],links=[],revoked=[];
globalThis.document={
  body:{append(){}},
  createElement(tag){
    assert.equal(tag,'a');
    const link={click(){this.clicked=true;},remove(){this.removed=true;}};
    links.push(link);return link;
  }
};
const originalCreate=URL.createObjectURL,originalRevoke=URL.revokeObjectURL;
const originalTimer=globalThis.setTimeout;
URL.createObjectURL=blob=>{blobs.push(blob);return `blob:material-${blobs.length}`;};
URL.revokeObjectURL=url=>revoked.push(url);
globalThis.setTimeout=(fn,delay)=>{assert.equal(delay,1000);fn();return 0;};
try{
  for(const [name,type] of [
    ['problem.pdf','application/pdf'],
    ['peopleQ3.csv','text/csv;charset=utf-8'],
    ['certificate.png','image/png']
  ]){
    const bytes=new Uint8Array([0,10,255,65]);
    bridge.downloadFile(name,bytes);
    assert.equal(blobs.at(-1).type,type);
    assert.deepEqual(new Uint8Array(await blobs.at(-1).arrayBuffer()),bytes);
    assert.equal(links.at(-1).download,name);
    assert.equal(links.at(-1).clicked,true);
    assert.equal(links.at(-1).removed,true);
  }
  assert.equal(revoked.length,3);
}finally{
  URL.createObjectURL=originalCreate;URL.revokeObjectURL=originalRevoke;
  globalThis.setTimeout=originalTimer;
}
console.log('Materials passed: worker reuse/routing/recovery/cleanup and original-file downloads');

// CI runs this mode against the actual shipped WASM, without a browser or GPU.
const builtAt=process.argv.indexOf('--built');
if(builtAt>=0){
  assert.ok(process.argv[builtAt+1],'--built needs a directory containing pdf-worker.js and ui/');
  const root=pathToFileURL(path.resolve(process.argv[builtAt+1])+path.sep);
  const messages=[],reads=[];
  globalThis.postMessage=(message,transfer=[])=>messages.push(structuredClone(message,{transfer}));
  globalThis.fetch=async url=>{
    const location=new URL(url);
    reads.push(location.href);
    return new Response(await fs.readFile(location),{headers:{
      'content-type':location.pathname.endsWith('.wasm')?'application/wasm':'application/octet-stream'
    }});
  };
  await import(new URL('pdf-worker.js',root));
  let id=0;
  async function render(page,width){
    const request=++id;
    await globalThis.onmessage({data:{id:request,page,width}});
    const message=messages.at(-1);
    assert.equal(message.id,request);
    return message;
  }
  for(const [page,width,height] of [[0,612,792],[1,612,792],[8,918,1188]]){
    const start=performance.now();
    const result=await render(page,width);
    assert.equal(result.error,undefined);
    assert.ok(result.bytes instanceof Uint8Array);
    const header=new DataView(result.bytes.buffer,result.bytes.byteOffset,8);
    assert.equal(header.getUint32(0,true),width);
    assert.equal(header.getUint32(4,true),height);
    assert.equal(result.bytes.length,8+width*height*4);
    let ink=0;
    for(let at=8;at<result.bytes.length;at+=4){
      assert.equal(result.bytes[at+3],255);
      if(result.bytes[at]<200||result.bytes[at+1]<200||result.bytes[at+2]<200)ink++;
    }
    assert.ok(ink>1000,`page ${page+1} is blank`);
    console.log(`Bundled PDF page ${page+1}: ${width}×${height}, ${(performance.now()-start).toFixed(0)} ms`);
  }
  assert.match((await render(9,612)).error,/页码/);
  assert.match((await render(0,4000)).error,/尺寸/);
  assert.match((await render(0,0)).error,/请求/);
  assert.equal(reads.length,1,'PDF pages/fonts should be bundled; only the WASM module is fetched');
  assert.ok(reads[0].endsWith('/ui/flight_ui_bg.wasm'));
  console.log('Built PDF worker passed: original first/map/last pages, bounded input and one module load');
}
