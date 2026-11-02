export const RULES='prp-flight-v1';
export const PROFILE=256;
export function parseCSV(text, expected, maxRows=40000) {
  if(typeof text!=='string'||text.includes('\0')||text.length>10_000_000) throw Error('CSV 文件为空、含 NUL 或超过 10 MB 限制');
  text=text.replace(/^\uFEFF/,'');
  const rows=[]; let row=[],field='',quoted=false,closed=false;
  for(let i=0;i<text.length;i++){
    const c=text[i];
    if(quoted){ if(c==='"'){if(text[i+1]==='"'){field+='"';i++;}else{quoted=false;closed=true;}}else field+=c; }
    else if(c==='"'){if(field||closed)throw Error('CSV 引号格式错误');quoted=true;}
    else if(c===','||c==='\n'||c==='\r'){
      row.push(field);field='';closed=false;
      if(c!==','){if(c==='\r'&&text[i+1]==='\n')i++;if(!(row.length===1&&row[0]===''))rows.push(row);row=[];}
    } else {if(closed)throw Error('CSV 引号后的内容无效');field+=c;}
  }
  if(quoted)throw Error('CSV 引号未闭合');
  if(field||row.length||closed){row.push(field);rows.push(row);}
  const header=rows.shift();
  if(!header||header.join(',')!==expected.join(','))throw Error(`表头必须为：${expected.join(',')}`);
  if(rows.length>maxRows)throw Error(`数据行超过 ${maxRows} 行`);
  if(rows.some(r=>r.length!==expected.length))throw Error('CSV 数据列数与表头不一致');
  return rows.map((r,i)=>Object.assign(Object.fromEntries(header.map((h,j)=>[h,r[j]])),{_line:i+2}));
}
function integer(v,label,min=0){if(!/^(0|[1-9]\d*)$/.test(v)||!Number.isSafeInteger(+v)||+v<min)throw Error(`${label} 应为${min?'正':'非负'}整数`);return +v;}
function minute(v,label){
  if(!/^\d{4}-\d\d-\d\d \d\d:\d\d$/.test(v))throw Error(`${label} 必须为 YYYY-MM-DD HH:MM`);
  const d=new Date(v.replace(' ','T')+':00Z');
  if(!Number.isFinite(+d)||d.toISOString().slice(0,16).replace('T',' ')!==v)throw Error(`${label} 不是有效日期时间`);
  return (+d-Date.UTC(2026,7,3))/60000;
}
const matches=(e,p)=> e===55?p<3:e===p;
export function checkSolution(D,q,routeText,assignmentText,{baseline=false}={}){
 try {
  if(![1,2,3].includes(q))throw Error('不支持的题号');
  const people=D.people[q], rf=q===3?'aircraft_id':'aircraft_type';
  const rh=[rf,'flight_no','stop_order','facility_id',...(q===3?['arrival_time','departure_time']:[]),'refuel'];
  const ah=['person_id',rf,'flight_no','pickup_stop_order','delivery_stop_order'];
  const rr=parseCSV(routeText,rh),aa=parseCSV(assignmentText,ah,people.length);
  if(!rr.length||!aa.length)throw Error('航线表和人员分配表都不能为空');
  const resources=q===3?D.planes:['T1','T2','T3'];
  const routes=new Map();
  for(const r of rr){
    const label=`航线表第 ${r._line} 行`, resource=resources.indexOf(r[rf]);
    if(resource<0)throw Error(`${label}：未知机型或飞机 ${r[rf]}`);
    const no=integer(r.flight_no,`${label} flight_no`,1), k=integer(r.stop_order,`${label} stop_order`), loc=D.locations.indexOf(r.facility_id);
    if(loc<0)throw Error(`${label}：未知地点 ${r.facility_id}`);
    if(!['0','1'].includes(r.refuel))throw Error(`${label}：refuel 必须填 0 或 1`);
    const refuel=r.refuel==='1';
    if(refuel&&!D.refuel.includes(loc))throw Error(`${label}：该地点不允许加油`);
    const key=`${resource}/${no}`;
    if(!routes.has(key))routes.set(key,{resource,no,key,stops:[]});
    const route=routes.get(key);
    if(route.stops.some(s=>s.k===k))throw Error(`${label}：停靠序号重复`);
    route.stops.push({k,loc,refuel,arr:q===3&&r.arrival_time!==''?minute(r.arrival_time,`${label} arrival_time`):null,dep:q===3&&r.departure_time!==''?minute(r.departure_time,`${label} departure_time`):null});
  }
  const flights=[...routes.values()].sort((a,b)=>a.resource-b.resource||a.no-b.no),counts={},lastEnd={};
  const totals={minutes:0,personMinutes:0,fuel10:0,flights:flights.length,landings:0,passengerKm:0,seatKm:0,temporary:0,emptyKm:0,emptyLegs:0,served:0};
  for(const [f,route] of flights.entries()){
    const {resource,no,stops}=route; route.index=f;
    const label=`${resources[resource]} / ${no}`;
    counts[resource]=(counts[resource]||0)+1;
    if(no!==counts[resource])throw Error(`${label}：flight_no 必须从 1 连续编号`);
    stops.sort((a,b)=>a.k-b.k);
    if(stops.some((s,i)=>s.k!==i))throw Error(`${label}：stop_order 必须从 0 连续编号`);
    if(stops.length<3||stops.length>7)throw Error(`${label}：每架次须有 1–5 次海上停靠`);
    const base=stops[0].loc,end=stops.at(-1);
    if(base>=3||end.loc!==base)throw Error(`${label}：必须从同一机场出发并返回`);
    if(q===3&&base!==D.planeHome[resource])throw Error(`${label}：必须从所属机场出发`);
    if(stops.slice(1,-1).some(s=>s.loc<3))throw Error(`${label}：中途停靠必须为海上设施`);
    const t=q===3?D.planeType[resource]:resource; route.type=t;
    let remaining=D.tank10[t];
    route.board=Array(stops.length).fill(0);route.alight=Array(stops.length).fill(0);
    for(let k=0;k<stops.length;k++){
      const s=stops[k],last=k===stops.length-1;
      if(q===3){
        if(k===0?(s.arr!==null||s.dep===null):last?(s.arr===null||s.dep!==null):(s.arr===null||s.dep===null))throw Error(`${label} 停靠 ${k}：到达/离开时刻填写不完整（首到达、末离开留空）`);
        if([s.arr,s.dep].some(v=>v!==null&&(v<0||v>10080)))throw Error(`${label}：时刻超出规划期`);
      }else if(k===0){s.arr=0;s.dep=0;}
      if(k){
        const prev=stops[k-1]; if(prev.loc===s.loc)throw Error(`${label}：连续停靠同一地点`);
        const d=D.distance[prev.loc][s.loc]; remaining-=d*D.fuelRate10[t];totals.fuel10+=d*D.fuelRate10[t];
        if(remaining<D.reserve10[t])throw Error(`${label} 航段 ${k-1}→${k}：到达余油低于安全余油`);
        if(s.refuel)remaining=D.tank10[t];
        const expected=prev.dep+Math.ceil(60*d/D.speed[t]);
        if(q===3&&s.arr!==expected)throw Error(`${label} 停靠 ${k}：到达时刻应为规划期第 ${expected} 分钟`);
        if(q!==3){s.arr=expected;s.dep=expected+(last?0:s.refuel?20:10);}
        if(!last&&s.dep-s.arr<(s.refuel?20:10))throw Error(`${label} 停靠 ${k}：停留不足 ${s.refuel?20:10} 分钟`);
      }
    }
    if(q===3){
      const start=stops[0].dep,finish=end.arr;
      if(start%1440<360||start%1440>1080)throw Error(`${label}：须在 06:00–18:00 起飞`);
      if(Math.floor(start/1440)!==Math.floor(finish/1440)||finish%1440>1200)throw Error(`${label}：须在当天 20:00 前返回`);
      if(lastEnd[resource]!==undefined&&start<lastEnd[resource]+30)throw Error(`${label}：与上一架次冲突或机场周转不足 30 分钟`);
      lastEnd[resource]=finish;
    }
    totals.minutes+=end.arr-stops[0].dep; totals.landings+=stops.length-2;
  }
  const byId=new Map(people.map((p,i)=>[p.id,{...p,index:i}])),seen=new Set(),assignments=Array(people.length);
  for(const r of aa){
    const p=byId.get(r.person_id),label=`人员 ${r.person_id}`;
    if(!p)throw Error(`${label} 不属于本问数据`);
    if(seen.has(p.id))throw Error(`${label} 重复出现`);seen.add(p.id);
    const blank=[rf,'flight_no','pickup_stop_order','delivery_stop_order'].map(h=>r[h]==='');
    if(blank.every(Boolean)){
      if(q!==3||p.task!=='temporary')throw Error(`${label} 为必运人员，不能留空`);
      assignments[p.index]={served:false,f:0,pickup:0,delivery:0};continue;
    }
    if(blank.some(Boolean))throw Error(`${label}：四个安排字段须同时填写或留空`);
    if(baseline&&p.task==='temporary')throw Error(`${label}：基础方案不得安排临时任务`);
    const resource=resources.indexOf(r[rf]),no=integer(r.flight_no,`${label} flight_no`,1),route=routes.get(`${resource}/${no}`);
    if(!route)throw Error(`${label} 引用了不存在的架次`);
    const a=integer(r.pickup_stop_order,`${label} pickup_stop_order`),b=integer(r.delivery_stop_order,`${label} delivery_stop_order`),stops=route.stops;
    if(a>=b||b>=stops.length)throw Error(`${label}：须先接后送，且停靠序号在架次范围内`);
    if(!matches(p.origin,stops[a].loc)||!matches(p.destination,stops[b].loc))throw Error(`${label}：接送地点与官方需求不一致`);
    for(let k=a+1;k<b;k++)if(matches(p.destination,stops[k].loc))throw Error(`${label}：必须在首次到达目的地时下机`);
    if(q===3&&(stops[a].dep<p.earliest||stops[b].arr>p.latest))throw Error(`${label}：不满足时间窗`);
    route.board[a]++;route.alight[b]++;
    totals.personMinutes+=stops[b].arr-stops[a].dep;totals.served++;
    if(p.task==='temporary')totals.temporary++;
    assignments[p.index]={served:true,f:route.index,pickup:a,delivery:b};
  }
  if(seen.size!==people.length)throw Error(`遗漏 ${people.length-seen.size} 名人员；所有人员（包括未服务的临时人员）都必须有一行`);
  for(const route of flights){
    let load=0;const {stops,type:t}=route;
    for(let k=0;k<stops.length;k++){
      load-=route.alight[k];if(load<0)throw Error('出现未登机先下机');load+=route.board[k];
      if(load>D.capacity[t])throw Error(`${resources[route.resource]} / ${route.no} 停靠 ${k}：载客 ${load}，超过 ${D.capacity[t]} 座`);
      if(k+1<stops.length){const d=D.distance[stops[k].loc][stops[k+1].loc];totals.passengerKm+=load*d;totals.seatKm+=D.capacity[t]*d;if(!load){totals.emptyKm+=d;totals.emptyLegs++;}}
    }
    if(load!==0)throw Error('返场后仍有人员未下机');
  }
  return {ok:true,q,baseline,totals,flights,assignments,score:[totals.minutes,totals.personMinutes,totals.fuel10,totals.flights,totals.landings,totals.passengerKm,totals.seatKm,totals.temporary,totals.emptyKm]};
 } catch(e){return {ok:false,errors:[e.message]};}
}
export function toWitness(result,identity){
  if(!result.ok)throw Error('必须先通过校验');
  if(result.flights.length>PROFILE)throw Error(`当前证书支持最多 ${PROFILE} 架次，此方案有 ${result.flights.length} 架次。`);
  const w={identity:Array.from(identity),mode:result.baseline?0:1,score:result.score,resources:Array(PROFILE).fill(0),lengths:Array(PROFILE).fill(0),loc:Array(PROFILE*7).fill(0),refuel:Array(PROFILE*7).fill(false),times_in:Array(PROFILE*7).fill(0),times_out:Array(PROFILE*7).fill(0),served:[],flight:[],pickup:[],delivery:[]};
  result.flights.forEach((r,f)=>{w.resources[f]=r.resource;w.lengths[f]=r.stops.length;r.stops.forEach((s,k)=>{let i=f*7+k;w.loc[i]=s.loc;w.refuel[i]=s.refuel;if(result.q===3){w.times_in[i]=s.arr??0;w.times_out[i]=s.dep??0;}});});
  result.assignments.forEach(a=>{w.served.push(a.served);w.flight.push(a.f);w.pickup.push(a.pickup);w.delivery.push(a.delivery);});
  return w;
}
