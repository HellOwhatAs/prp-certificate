"""Create five portable downloads: source, Pages and three public parameter bundles."""
from pathlib import Path
import hashlib
import json
import zipfile

out=Path('release')
out.mkdir(exist_ok=True)
manifest=json.loads(Path('public/groth16/manifest.json').read_text())
for q in range(1,4):
    info=manifest['circuits'][str(q)]
    full_hash=hashlib.sha256()
    archive=out/f'prp-flight-parameters-q{q}.zip'
    with zipfile.ZipFile(archive,'w',compression=zipfile.ZIP_STORED) as z:
        for chunk in info['chunks']:
            file=Path('public/groth16')/chunk['file']
            data=file.read_bytes()
            if len(data)!=chunk['bytes'] or hashlib.sha256(data).hexdigest()!=chunk['sha256']:
                raise SystemExit(f'Invalid public parameter chunk: {file}')
            full_hash.update(data)
            z.write(file,file.as_posix())
        if full_hash.hexdigest()!=info['pk_sha256']:
            raise SystemExit(f'Invalid complete Q{q} parameters')
    print(archive.resolve())

folders=['.github','assets','circuits','crates','public','scripts','src','tests','web','docs']
root_files=['.gitignore','.gitattributes','Cargo.toml','Cargo.lock','rust-toolchain.toml','package.json','package-lock.json']
files=[Path(name) for name in root_files if Path(name).is_file()]
files+=list(Path('.').glob('*.md'))
for folder in folders:
    if Path(folder).is_dir():
        files.extend(p for p in Path(folder).rglob('*') if p.is_file() and p.suffix!='.dat' and '__pycache__' not in p.parts and not p.name.startswith('.q'))
archive=out/'prp-certificate-source.zip'
with zipfile.ZipFile(archive,'w',compression=zipfile.ZIP_DEFLATED,compresslevel=6) as z:
    for file in sorted(set(files)):
        z.write(file,Path('prp-certificate')/file)
print(archive.resolve())

site=Path('dist/client')
if not (site/'deployment.json').is_file():
    raise SystemExit('Run npm run build:pages before packaging')
archive=out/'prp-certificate-pages.zip'
with zipfile.ZipFile(archive,'w',compression=zipfile.ZIP_DEFLATED,compresslevel=6) as z:
    for file in sorted(site.rglob('*')):
        if file.is_file():z.write(file,file.relative_to(site))
print(archive.resolve())
