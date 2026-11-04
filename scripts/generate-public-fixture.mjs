// Deliberately ordinary, reproducible Q1 fixture made only from the public data.
// One destination per flight, T3 only, A01 for passengers without a fixed base.
// This is a feasibility regression, not an optimizer or a competition solution.
import fs from 'node:fs/promises';
import path from 'node:path';
import assert from 'node:assert/strict';
import {checkSolution, PROFILE} from '../src/checker.js';

const args = process.argv.slice(2);
assert.ok(args.every((arg, i) => ['--check', '--write', '--job'].includes(arg) || args[i - 1] === '--job'), 'Usage: node scripts/generate-public-fixture.mjs [--check|--write] [--job output.json]');
assert.ok(!(args.includes('--check') && args.includes('--write')), 'Choose --check or --write');
const jobIndex = args.indexOf('--job');
assert.ok(jobIndex < 0 || (args[jobIndex + 1] && !args[jobIndex + 1].startsWith('--')), '--job needs an output path');
const data = JSON.parse(await fs.readFile('public/data/instance.json'));
const type = 2, capacity = data.capacity[type], refuel = new Set(data.refuel);

function feasible(stops) {
  let fuel = data.tank10[type];
  for (let i = 1; i < stops.length; i++) {
    if (stops[i - 1] === stops[i]) return false;
    fuel -= data.distance[stops[i - 1]][stops[i]] * data.fuelRate10[type];
    if (fuel < data.reserve10[type]) return false;
    if (refuel.has(stops[i])) fuel = data.tank10[type];
  }
  return true;
}

function route(base, destination) {
  // At most one refuelling stop on either side of the destination is sufficient
  // for this public instance. Prefer fewer stops, then distance, then IDs.
  const candidates = [[base, destination, base]];
  for (const a of data.refuel) {
    candidates.push([base, a, destination, base], [base, destination, a, base]);
    for (const b of data.refuel) candidates.push([base, a, destination, b, base]);
  }
  const distance = stops => stops.slice(1).reduce((sum, next, i) => sum + data.distance[stops[i]][next], 0);
  const routes = candidates.filter(feasible).sort((a, b) => a.length - b.length || distance(a) - distance(b) || a.join(',').localeCompare(b.join(','), 'en'));
  assert.ok(routes.length, `No simple feasible route from ${base} to ${destination}`);
  return routes[0];
}

const groups = new Map();
for (const person of data.people[1]) {
  const base = person.origin === 55 ? 0 : person.origin;
  assert.ok(base < 3 && person.destination >= 3);
  const key = `${base}/${person.destination}`;
  if (!groups.has(key)) groups.set(key, {base, destination:person.destination, people:[]});
  groups.get(key).people.push(person.id);
}
const routes = ['aircraft_type,flight_no,stop_order,facility_id,refuel'];
const assignments = new Map();
let flights = 0;
for (const group of [...groups.values()].sort((a, b) => a.base - b.base || a.destination - b.destination)) {
  const stops = route(group.base, group.destination);
  for (let at = 0; at < group.people.length; at += capacity) {
    const number = ++flights;
    stops.forEach((loc, k) => routes.push(`T3,${number},${k},${data.locations[loc]},${Number(refuel.has(loc))}`));
    for (const id of group.people.slice(at, at + capacity)) assignments.set(id, `${id},T3,${number},0,${stops.indexOf(group.destination)}`);
  }
}
assert.ok(flights <= PROFILE, `Fixture exceeds the ${PROFILE}-flight proof bound`);
const routeText = routes.join('\n') + '\n';
const assignmentText = ['person_id,aircraft_type,flight_no,pickup_stop_order,delivery_stop_order', ...data.people[1].map(p => assignments.get(p.id))].join('\n') + '\n';
const checked = checkSolution(data, 1, routeText, assignmentText);
assert.equal(checked.ok, true, checked.errors?.join('\n'));
const expected = {question:1, dataset:data.datasetId, scores:checked.score, served:checked.totals.served};
const folder = 'tests/fixtures/public';
const files = {'q1-routes.csv':routeText, 'q1-assignments.csv':assignmentText, 'q1-expected.json':JSON.stringify(expected, null, 2) + '\n'};
if (args.includes('--write')) await fs.mkdir(folder, {recursive:true});
for (const [name, text] of Object.entries(files)) {
  const file = `${folder}/${name}`;
  if (args.includes('--write')) await fs.writeFile(file, text);
  else assert.equal(await fs.readFile(file, 'utf8'), text, `${file} differs from the deterministic public fixture`);
}
if (jobIndex >= 0) {
  const output = args[jobIndex + 1];
  await fs.mkdir(path.dirname(output), {recursive:true});
  await fs.writeFile(output, JSON.stringify({action:'prove', q:1, team:'Public regression fixture', files:{routes:routeText, assignments:assignmentText}}));
}
console.log(`Public Q1 fixture: ${checked.totals.served} people, ${flights} flights, ${checked.score[0]} aircraft minutes; ${args.includes('--write') ? 'written' : 'reproduction checked'}`);
