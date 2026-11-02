"""Verify private PRG2 proof fixtures; no solutions or fixtures are shipped.
Usage: python scripts/test-certificates.py .cache/combined-proofs/*.json
"""
import base64, copy, json, subprocess, sys
from pathlib import Path
binary = Path('target/release/flight-ui').resolve()
def verify(certificate):
    p = subprocess.run([str(binary),'--engine'], input=json.dumps({'action':'verify','certificate':certificate}), text=True, capture_output=True, check=True)
    return json.loads(p.stdout.strip().splitlines()[-1])
for filename in sys.argv[1:]:
    cert=json.loads(Path(filename).read_text())['certificate']
    assert verify(cert)['type']=='result', filename
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
    for i,c in enumerate(changes):assert verify(c)['type']=='error',(filename,i)
    # A valid first question cannot hide an invalid second proof.
    if cert['certificates'][0]['metadata']['question']==1:
        c=copy.deepcopy(cert);second=copy.deepcopy(c['certificates'][0]);second['metadata']['question']=2
        c['certificates'].append(second)
        r=verify(c);assert r['type']=='error' and '第 2 问' in r['message'],r
    print(f'{Path(filename).name}: valid set accepted; {len(changes)} tamper/schema cases rejected; invalid added question rejects the set')
