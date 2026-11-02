"""Check existing private full-proof outputs against the shipped Rust verifier.

Usage: python scripts/test-certificates.py .cache/rust-tests/*-prove.json
"""
import base64
import copy
import hashlib
import json
from pathlib import Path
import subprocess
import sys

binary = Path('target/release/flight-engine').resolve()
def verify(certificate):
    p = subprocess.run([str(binary)], input=json.dumps({'action': 'verify', 'certificate': certificate}), text=True, capture_output=True, check=True)
    return json.loads(p.stdout.strip().splitlines()[-1])['type'] == 'result'

for filename in sys.argv[1:]:
    cert = json.loads(Path(filename).read_text())['certificate']
    assert verify(cert), filename
    changes = []
    bad = copy.deepcopy(cert); bad['metadata']['team'] = '篡改队名'; changes.append(bad)
    bad = copy.deepcopy(cert); bad['metadata']['extra'] = 1; changes.append(bad)
    for index in range(9):
        bad=copy.deepcopy(cert);bad['scores'][index]+=1;changes.append(bad)
    for key in ['format','rules','dataset','nonce']:
        bad=copy.deepcopy(cert);bad['metadata'][key]+='x';changes.append(bad)
    bad=copy.deepcopy(cert);bad['metadata']['question']=1 if cert['metadata']['question']!=1 else 2;changes.append(bad)
    bad=copy.deepcopy(cert);proof=bytearray(base64.b64decode(bad['proof']));proof[-1]^=1;bad['proof']=base64.b64encode(proof).decode();changes.append(bad)
    bad=copy.deepcopy(cert);bad['proof']='';changes.append(bad)
    bad=copy.deepcopy(cert);bad['vk']='untrusted';changes.append(bad)
    for index, bad in enumerate(changes):
        assert not verify(bad), f'{filename}: tamper {index} accepted'
    print(f'{Path(filename).name}: valid proof accepted; {len(changes)} tamper cases rejected')
