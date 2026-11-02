import fs from 'node:fs/promises';
import assert from 'node:assert/strict';
import {checkSolution,toWitness,parseCSV} from '../src/checker.js';
const D=JSON.parse(await fs.readFile('public/data/instance.json'));
const dir=process.env.REFERENCE_DIR;
const testDir=process.env.ZK_TEST_DIR||'.cache/zk-tests';
assert.throws(()=>parseCSV('a,b\n1,2,3',['a','b']));
assert.throws(()=>parseCSV('a,b\n"bad',['a','b']));
assert.deepEqual(parseCSV('\uFEFFa,b\r\n"1","2"\r\n',['a','b'])[0],{a:'1',b:'2',_line:2});
if(dir){
 for(const q of [1,2,3]){
  const r=await fs.readFile(`${dir}/q${q}-routes.csv`,'utf8'),a=await fs.readFile(`${dir}/q${q}-assignments.csv`,'utf8');
  const result=checkSolution(D,q,r,a);assert.equal(result.ok,true,result.errors?.join('\n'));
  assert.equal(result.totals.minutes,[0,14950,17918,27161][q]);
  const old=JSON.parse(await fs.readFile(`${dir}/reports/metrics_q${q}.json`)).metrics;
  for(const [k,j] of [['minutes','aircraft_minutes'],['personMinutes','person_minutes'],['flights','flight_count'],['landings','landings'],['passengerKm','passenger_km'],['seatKm','available_seat_km'],['emptyKm','empty_leg_km']])assert.equal(result.totals[k],old[j],`${q}:${k}`);
  assert.equal(result.totals.fuel10/10,old.fuel_kg);
  assert.equal(toWitness(result,new Uint8Array(32)).served.length,D.people[q].length);
  assert.equal(checkSolution(D,q,r,a+a.split('\n')[1]+'\n').ok,false,'duplicate person must fail');
  assert.equal(checkSolution(D,q,r,a.replace(/P\d{4}/,'NOT_A_PERSON')).ok,false);
  const first=a.trim().split('\n');first.pop();assert.equal(checkSolution(D,q,r,first.join('\n')).ok,false);
  assert.equal(checkSolution(D,q,r.replace(/,F\d{3},/g,',A01,'),a).ok,false,'bad route');
  if(q===3){assert.equal(checkSolution(D,q,r,a,{baseline:true}).ok,false);}
  await fs.mkdir(testDir,{recursive:true});
  await fs.writeFile(`${testDir}/q${q}-witness.json`,JSON.stringify(toWitness(result,new Uint8Array(32))));
  console.log(`Q${q}: original metrics match; malformed solutions rejected`);
 }
}else console.log('CSV tests passed. Set REFERENCE_DIR to the private trial_solve folder for full regression.');
