// Invalid solutions must fail before the Worker loads a large parameter bundle.
import fs from 'node:fs/promises';
import assert from 'node:assert/strict';
let result,manifestReads=0,bundleReads=0,componentReads=0;
globalThis.postMessage=message=>{if(message.type!=='progress')result=message;};
globalThis.onmessage=null;
globalThis.fetch=async url=>{
 if(String(url).includes('/parameters/')){componentReads++;return new Response('',{status:503});}
 if(String(url).endsWith('manifest.json'))manifestReads++;
 return new Response(await fs.readFile(new URL(url)),{
  headers:{'content-type':String(url).endsWith('.wasm')?'application/wasm':'application/octet-stream'}
 });
};
await import('../dist/client/zk-worker.js');
await onmessage({data:{action:'prove',q:1,team:'test',
 files:{routes:'invalid',assignments:'invalid'},
 parameterFile:{size:1,stream(){bundleReads++;throw Error('unexpected parameter read');}}
}});
assert.equal(result.type,'error');
assert.match(result.message,/CSV|表头|字段|航线|列/);
assert.equal(manifestReads,0);assert.equal(bundleReads,0);
console.log('Invalid CSV rejected before manifest or parameter reads');
// A small public-data route reaches assignment validation without private solutions.
const d=JSON.parse(await fs.readFile('public/data/instance.json'));
const canReturn=(i,s)=>s>=3&&2*d.distance[d.planeHome[i]][s]*d.fuelRate10[d.planeType[i]]<=d.tank10[d.planeType[i]]-d.reserve10[d.planeType[i]];
const aircraft=d.planes.findIndex((_,i)=>d.locations.some((_,s)=>canReturn(i,s)));
assert.ok(aircraft>=0);
const plane=d.planes[aircraft],home=d.planeHome[aircraft],type=d.planeType[aircraft];
const stop=d.locations.map((_,i)=>i).filter(i=>canReturn(aircraft,i)).sort((a,b)=>d.distance[home][a]-d.distance[home][b])[0];
const flight=Math.ceil(60*d.distance[home][stop]/d.speed[type]);
const time=m=>new Date(Date.UTC(2026,7,3,0,m)).toISOString().slice(0,16).replace('T',' ');
const routes=[
 'aircraft_id,flight_no,stop_order,facility_id,arrival_time,departure_time,refuel',
 `${plane},1,0,${d.locations[home]},,${time(360)},0`,
 `${plane},1,1,${d.locations[stop]},${time(360+flight)},${time(370+flight)},0`,
 `${plane},1,2,${d.locations[home]},${time(370+2*flight)},,0`
].join('\n');
const header='person_id,aircraft_id,flight_no,pickup_stop_order,delivery_stop_order\n';
const temporary=d.people[3].find(p=>p.task==='temporary').id;
const required=d.people[3].find(p=>p.task!=='temporary').id;
for(const action of ['check','prove']){
 for(const [assignments,expected,person] of [
  ['invalid',/人员分配表：表头/,''],
  [header+`${temporary},${plane},,,`,/填写不完整.*全部留空/,temporary],
  [header+`${required},,,,`,/必运人员.*不能为空/,required],
  [header+`${temporary},,,,`,/遗漏.*全部人员记录.*保留 person_id/,'']
 ]){
  await onmessage({data:{action,q:3,team:'test',files:{routes,assignments}}});
  assert.equal(result.type,'error');assert.match(result.message,expected);
  if(person)assert.ok(result.message.includes(person));
 }
}
assert.equal(manifestReads,0);assert.equal(bundleReads,0);assert.equal(componentReads,0);
console.log('Q3 check/prove return actionable table, person and assignment errors before downloads');
if(process.argv[2]){
 const job=JSON.parse(await fs.readFile(process.argv[2]));job.action='prove';
 await onmessage({data:job});
 assert.equal(result.type,'error');assert.match(result.message,/证明组件/);
 assert.equal(manifestReads,1);assert.equal(bundleReads,0);assert.ok(componentReads>0);
 console.log('Valid solution automatically requests components without a selected file');
}
