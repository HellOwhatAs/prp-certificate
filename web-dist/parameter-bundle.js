// Browser transport only: stream the published tar.gz into its original zlib key.
// Rust authenticates the complete key against its embedded manifest before use.
export async function readParameterBundle(file,info,progress=()=>{}) {
 if(!file||typeof file.stream!=='function'||file.size>600_000_000)throw Error('请先选择本题的参数包（tar.gz）');
 if(!Number.isSafeInteger(info?.pk_bytes)||info.pk_bytes<=0||info.pk_bytes>600_000_000)throw Error('公开参数清单无效');
 const parts=new Map();let offset=0;
 for(const p of info.chunks){if(!/^q[123]-[0-9]+\.dat$/.test(p.file)||!Number.isSafeInteger(p.bytes)||p.bytes<1||p.bytes>4194304||parts.has(p.file))throw Error('公开参数清单无效');parts.set(p.file,{...p,offset});offset+=p.bytes;}
 if(offset!==info.pk_bytes)throw Error('公开参数长度无效');
 const out=new Uint8Array(info.pk_bytes),seen=new Set();
 const reader=file.stream().pipeThrough(new DecompressionStream('gzip')).getReader();
 let current=new Uint8Array(),at=0,total=0;
 const cap=info.pk_bytes+info.chunks.length*4096+32768;
 async function take(n,target,position=0){
  let left=n;
  while(left){if(at===current.length){const r=await reader.read();if(r.done)throw Error('参数包不完整');current=r.value;at=0;total+=current.length;if(total>cap)throw Error('参数包解压大小超限');}
   const count=Math.min(left,current.length-at);if(target)target.set(current.subarray(at,at+count),position);position+=count;at+=count;left-=count;
  }
 }
 const string=b=>new TextDecoder().decode(b).replace(/\0.*$/s,'').trim();
 const octal=b=>{const s=string(b);if(!/^[0-7]+$/.test(s))throw Error('参数包头部无效');const v=Number.parseInt(s,8);if(!Number.isSafeInteger(v))throw Error('参数包头部无效');return v;};
 try {
  for(;;){
   const header=new Uint8Array(512);await take(512,header);
   if(header.every(x=>x===0))break;
   const checksum=header.reduce((sum,v,i)=>sum+(i>=148&&i<156?32:v),0);
   if(checksum!==octal(header.subarray(148,156)))throw Error('参数包头部校验失败');
   const size=octal(header.subarray(124,136)),kind=header[156],name=string(header.subarray(0,100));
   if(kind===120||kind===103||kind===53){if(size>1048576)throw Error('参数包附加信息过大');await take(size);}
   else if(kind===0||kind===48){
    const part=parts.get(name.split('/').at(-1));
    if(!part||seen.has(part.file)||size!==part.bytes)throw Error('参数包题号或内容不匹配');
    await take(size,out,part.offset);seen.add(part.file);progress(`读取参数包 ${seen.size} / ${parts.size}…`);
   }else throw Error('参数包格式不受支持');
   if(size%512)await take(512-size%512);
  }
  // Consume padding to validate the gzip checksum and reject trailing archives.
  for(;;){if(current.subarray(at).some(x=>x!==0))throw Error('参数包含有多余内容');const r=await reader.read();if(r.done)break;current=r.value;at=0;total+=current.length;if(total>cap)throw Error('参数包解压大小超限');}
  if(seen.size!==parts.size)throw Error('参数包不完整');
  return out;
 }finally{await reader.cancel().catch(()=>{});reader.releaseLock();}
}
