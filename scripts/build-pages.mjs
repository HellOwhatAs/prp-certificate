// GitHub Pages serves the small app. GitHub Raw serves pinned public key chunks.
import fs from 'node:fs/promises';
function option(name, fallback) {
  const i=process.argv.indexOf(name);
  return i<0?fallback:process.argv[i+1];
}
const repository=option('--repository',process.env.GITHUB_REPOSITORY);
const ref=option('--ref',process.env.GITHUB_SHA);
if(!/^[A-Za-z0-9_.-]+\/[A-Za-z0-9_.-]+$/.test(repository??''))
  throw Error('请指定公开仓库：npm run build:pages -- --repository OWNER/REPO --ref main');
if(!/^[A-Za-z0-9_.\/-]+$/.test(ref??'')||ref.includes('..')||ref.startsWith('/')||ref.endsWith('/'))
  throw Error('请通过 --ref 指定提交 SHA、分支或标签');
await import('./build.mjs');
const base=`https://raw.githubusercontent.com/${repository}/${ref}/public/groth16/`;
const target='dist/client/parameters.js';
const source=await fs.readFile(target,'utf8');
const marker="const PARAMETER_BASE_URL = '';";
if(source.split(marker).length!==2)throw Error('网页参数地址标记缺失或重复');
await fs.writeFile(target,source.replace(marker,`const PARAMETER_BASE_URL = ${JSON.stringify(base)};`));
await fs.writeFile('dist/client/deployment.json',JSON.stringify({repository,ref,parameterBaseUrl:base},null,2)+'\n');
console.log(`Pages: dist/client/; public parameters: ${base}`);
