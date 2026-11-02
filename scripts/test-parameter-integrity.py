"""Release check: canonical dataset, circuit manifests, public key bytes and hashes."""
from pathlib import Path
import hashlib,json,base64
root=Path('public/groth16');manifest=json.loads((root/'manifest.json').read_text())
assert manifest['dataset']==Path('public/data/dataset-id.txt').read_text()
assert manifest['dataset']==json.loads(Path('public/data/instance.json').read_text())['datasetId']
for q in range(1,4):
 info=manifest['circuits'][str(q)];h=hashlib.sha256();size=0
 for chunk in info['chunks']:
  data=(root/chunk['file']).read_bytes();assert len(data)==chunk['bytes'];assert hashlib.sha256(data).hexdigest()==chunk['sha256'];h.update(data);size+=len(data)
 assert size==info['pk_bytes'];assert h.hexdigest()==info['pk_sha256']
 vk=base64.b64decode(info['vk'],validate=True);assert vk==(root/f'q{q}.vk').read_bytes();assert hashlib.sha256(vk).hexdigest()==info['vk_sha256']
 print(f'Q{q}: {size} bytes, all public parameter hashes and fixed VK verified')
