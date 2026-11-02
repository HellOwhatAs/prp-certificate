// Local preview with same-origin public parameter files. Pages uses GitHub Raw.
import http from 'node:http';
import fs from 'node:fs/promises';
import {createReadStream} from 'node:fs';
import path from 'node:path';
const root=path.resolve('dist/client');
const manifest=JSON.parse(await fs.readFile('public/groth16/manifest.json'));
const parameters=new Map(Object.values(manifest.circuits).flatMap(info=>info.chunks.map(part=>[`/parameters/${info.pk_sha256}/${part.file}`,path.resolve('public/groth16',part.file)])));
const types={'.html':'text/html; charset=utf-8','.js':'text/javascript; charset=utf-8','.wasm':'application/wasm','.json':'application/json','.css':'text/css','.png':'image/png','.svg':'image/svg+xml','.csv':'text/csv; charset=utf-8'};
const argument=(name,fallback)=>{const i=process.argv.indexOf(name);return i<0?fallback:process.argv[i+1];};
http.createServer(async(req,res)=>{
 try{
  if(!['GET','HEAD'].includes(req.method)){res.writeHead(405);res.end();return;}
  const pathname=decodeURIComponent(new URL(req.url,'http://preview.local').pathname);
  const file=parameters.get(pathname)??path.resolve(root,'.'+(pathname==='/'?'/index.html':pathname));
  if(!parameters.has(pathname)&&!file.startsWith(root+path.sep)){res.writeHead(404);res.end();return;}
  const stat=await fs.stat(file);if(!stat.isFile())throw Error('Not a file');
  res.writeHead(200,{'content-type':types[path.extname(file)]??'application/octet-stream','content-length':stat.size,'cache-control':'no-store'});
  if(req.method==='HEAD')res.end();else createReadStream(file).pipe(res);
 }catch{res.writeHead(404);res.end('Not found');}
}).listen(Number(argument('--port',process.env.PORT??3000)),argument('--host','0.0.0.0'));
