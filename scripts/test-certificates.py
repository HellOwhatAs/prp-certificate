"""Verify real fixed-key PRG2 certificates and reject forged claims.
Defaults to the public certificate fixtures. Extra local/private results may be
supplied explicitly; neither the certificates nor the solutions are logged.
Usage: python scripts/test-certificates.py [--binary path/to/flight-engine] [files...]
"""
import argparse, base64, copy, json, os, subprocess
from pathlib import Path
parser=argparse.ArgumentParser(description=__doc__)
parser.add_argument('--binary',type=Path,default=Path('target/release')/('flight-engine.exe' if os.name=='nt' else 'flight-engine'))
parser.add_argument('files',nargs='*',type=Path)
args=parser.parse_args()
binary=args.binary.resolve()
files=args.files or sorted(Path('tests/fixtures/certificates').glob('*.json'))
if not files: parser.error('No certificate fixtures were supplied or found')
if not binary.is_file(): parser.error('Build flight-engine or specify --binary before running this test')
def verify(certificate):
    p = subprocess.run([str(binary),'--engine'], input=json.dumps({'action':'verify','certificate':certificate}), text=True, capture_output=True, check=True, timeout=60)
    return json.loads(p.stdout.strip().splitlines()[-1])
for filename in files:
    original=json.loads(filename.read_text())
    cert=original['certificate']
    valid=verify(cert)
    assert valid['type']=='result', 'Valid fixed-key certificate rejected'
    assert valid['result']['scores']==original['scores'], 'Verified scores differ from the pinned fixture'
    changes=[]
    def change(f):
        c=copy.deepcopy(cert);f(c['certificates'][0]);changes.append(c)
    change(lambda c:c['metadata'].__setitem__('team','篡改队名'))
    change(lambda c:c['metadata'].__setitem__('extra',1))
    for i in range(9): change(lambda c:c['scores'].__setitem__(i,c['scores'][i]+1))
    for key in ['format','rules','dataset','nonce']: change(lambda c:c['metadata'].__setitem__(key,c['metadata'][key]+'x'))
    change(lambda c:c['metadata'].__setitem__('question',2 if c['metadata']['question']==1 else 1))
    def proof(c):
        b=bytearray(base64.b64decode(c['proof']));b[-1]^=1;c['proof']=base64.b64encode(b).decode()
    change(proof)
    change(lambda c:c.__setitem__('proof',''))
    change(lambda c:c.__setitem__('vk','untrusted'))
    c=copy.deepcopy(cert);c['certificates'].append(c['certificates'][0]);changes.append(c)
    c=copy.deepcopy(cert);c['certificates']=[];changes.append(c)
    c=copy.deepcopy(cert);c['extra']=1;changes.append(c)
    c=copy.deepcopy(cert);c['format']='unknown';changes.append(c)
    changes.append(cert['certificates'][0]) # Single-entry envelopes are not accepted.
    for i,c in enumerate(changes):assert verify(c)['type']=='error',f'Tamper/schema case {i} was accepted'
    # A valid first question cannot hide an invalid second proof.
    if cert['certificates'][0]['metadata']['question']==1:
        c=copy.deepcopy(cert);second=copy.deepcopy(c['certificates'][0]);second['metadata']['question']=2
        c['certificates'].append(second)
        r=verify(c);assert r['type']=='error' and '第 2 问' in r['message'],r
    print(f'{filename.name}: valid set accepted; {len(changes)} tamper/schema cases rejected; invalid added question rejects the set')
