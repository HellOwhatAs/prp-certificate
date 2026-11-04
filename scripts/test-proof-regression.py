"""Run real fixed-parameter generation and adversarial regressions.

Requires job-qN.json for EVERY requested question. Missing fixtures, binaries or
parameter chunks are errors. Defaults to all three questions; release CI selects
only the available public Q1 fixture explicitly. No solutions enter logs/reports.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import time

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('--fixtures', type=Path, required=True)
parser.add_argument('--questions', default='1,2,3')
parser.add_argument('--binary', type=Path, default=Path('target/release') / ('flight-engine.exe' if os.name == 'nt' else 'flight-engine'))
parser.add_argument('--circuit-binary', type=Path)
parser.add_argument('--parameters', type=Path, default=Path('public/groth16'))
parser.add_argument('--output', type=Path, default=Path('.cache/proof-regression'))
parser.add_argument('--timeout', type=int, default=1200)
args = parser.parse_args()
try:
    questions = [int(q) for q in args.questions.split(',')]
except ValueError:
    parser.error('--questions must be 1, 2, 3 or a comma-separated subset')
if not questions or questions != sorted(set(questions)) or any(q not in (1, 2, 3) for q in questions):
    parser.error('--questions must contain unique ascending question numbers from 1 to 3')
if args.timeout < 1:
    parser.error('--timeout must be positive')
binary = args.binary.resolve()
circuit_binary = (args.circuit_binary or binary.parent / 'examples' / ('groth_regression.exe' if os.name == 'nt' else 'groth_regression')).resolve()
for tool in (binary, circuit_binary):
    if not tool.is_file():
        parser.error(f'Missing {tool.name}; build flight-engine and the groth_regression example first')

# Read all required inputs before starting a costly proof. Only keep the schema
# the application needs, and replace any private fixture author label.
jobs = {}
for q in questions:
    try:
        value = json.loads((args.fixtures / f'job-q{q}.json').read_text())
        if value.get('q') != q or any(not isinstance(value['files'][name], str) or not value['files'][name] for name in ('routes', 'assignments')):
            raise ValueError('invalid fixture')
        jobs[q] = {'action':'prove', 'q':q, 'team':'Public regression fixture' if questions == [1] else 'Private regression fixture', 'files':{name:value['files'][name] for name in ('routes', 'assignments')}}
    except (OSError, ValueError, KeyError, TypeError):
        parser.error(f'Missing or invalid job-q{q}.json; requested questions are never silently skipped')

manifest = json.loads(Path('public/groth16/manifest.json').read_text())
args.output.mkdir(parents=True, exist_ok=True)
results = []

def execute(job, environment, q):
    try:
        run = subprocess.run([str(binary), '--engine'], input=json.dumps(job), text=True,
                             capture_output=True, timeout=args.timeout, env=environment, check=True)
        messages = [json.loads(line) for line in run.stdout.splitlines() if line.strip()]
    except (subprocess.SubprocessError, ValueError):
        raise SystemExit(f'Q{q}: native engine failed or timed out; private inputs were not logged') from None
    if not messages or messages[-1].get('type') != 'result':
        raise SystemExit(f'Q{q}: {job["action"]} failed; private inputs were not logged')
    if any('下载证明组件' in message.get('text', '') for message in messages):
        raise SystemExit(f'Q{q}: regression unexpectedly attempted a network parameter download')
    return messages[-1]['result']

with tempfile.TemporaryDirectory(prefix='prp-proof-regression-') as temporary:
    root = Path(temporary)
    environment = dict(os.environ, FLIGHT_CACHE_DIR=str(root / 'parameters'))
    checked_jobs = {}
    for q, job in jobs.items():
        checked_jobs[q] = execute(dict(job, action='check'), environment, q)
        if checked_jobs[q].get('canProve') is not True:
            raise SystemExit(f'Q{q}: fixture exceeds the proof profile')
    print('All requested fixtures passed the native checker and proof-size bound.', flush=True)
    for q in questions:
        info = manifest['circuits'][str(q)]
        destination = root / 'parameters' / info['pk_sha256']
        destination.mkdir(parents=True)
        total = hashlib.sha256()
        for part in info['chunks']:
            source = args.parameters / part['file']
            try:
                data = source.read_bytes()
            except OSError:
                raise SystemExit(f'Q{q}: missing public parameter chunk {part["file"]}') from None
            if len(data) != part['bytes'] or hashlib.sha256(data).hexdigest() != part['sha256']:
                raise SystemExit(f'Q{q}: invalid public parameter chunk {part["file"]}')
            total.update(data)
            try:
                os.link(source, destination / part['file'])
            except OSError:
                shutil.copyfile(source, destination / part['file'])
        if total.hexdigest() != info['pk_sha256']:
            raise SystemExit(f'Q{q}: complete public parameter hash mismatch')

    for q, job in jobs.items():
        started = time.monotonic()
        checked = checked_jobs[q]
        print(f'Q{q}: public parameters authenticated; checking circuit mutations.', flush=True)
        # The existing example checks a real satisfying assignment, identical
        # blank/witness matrices, and seven directly corrupted private witnesses.
        sanitized_job = root / f'job-q{q}.json'
        sanitized_job.write_text(json.dumps(job))
        try:
            subprocess.run([str(circuit_binary), str(sanitized_job)], capture_output=True,
                           timeout=args.timeout, check=True, env=environment)
        except subprocess.SubprocessError:
            raise SystemExit(f'Q{q}: circuit mutation regression failed; private inputs were not logged') from None
        print(f'Q{q}: circuit mutations rejected; generating and verifying a fixed-key proof.', flush=True)
        proved = execute(job, environment, q)
        if proved['scores'] != checked['scores']:
            raise SystemExit(f'Q{q}: proof scores differ from checker scores')
        verified = execute({'action':'verify', 'certificate':proved['certificate']}, environment, q)
        if verified != proved:
            raise SystemExit(f'Q{q}: verification differs from the generated certificate')
        output = args.output / f'q{q}.json'
        output.write_text(json.dumps(proved, indent=2) + '\n')
        # Exhaustive public-claim/schema mutations are kept in one shared test.
        subprocess.run([sys.executable, 'scripts/test-certificates.py', '--binary', str(binary), str(output)], check=True)
        seconds = round(time.monotonic() - started, 3)
        results.append({'question':q, 'passed':True, 'seconds':seconds})
        print(f'Q{q}: checker, circuit mutations, fixed-key proof, verification and tampering passed ({seconds:.1f}s)', flush=True)

(args.output / 'report.json').write_text(json.dumps({'questions':questions, 'results':results}, indent=2) + '\n')
missing = sorted(set((1, 2, 3)) - set(questions))
if missing:
    print('Not exercised by this run: ' + ', '.join(f'Q{q} full generation' for q in missing), flush=True)
