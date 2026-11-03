"""Package one native application; shared parameters are packaged once separately."""
from pathlib import Path
import argparse
import platform
import shutil
import tarfile
import tempfile
import zipfile

parser=argparse.ArgumentParser()
parser.add_argument('--target', help='Rust target triple; omitted for a local host build')
parser.add_argument('--label', help='Stable release label, e.g. windows-arm64')
args=parser.parse_args()
system={'darwin':'macos'}.get(platform.system().lower(),platform.system().lower())
arch={'x86_64':'x64','amd64':'x64','aarch64':'arm64','arm64':'arm64'}.get(platform.machine().lower(),platform.machine().lower())
label=args.label or f'{system}-{arch}'
if label not in {f'{s}-{a}' for s in ['linux','windows','macos'] for a in ['x64','arm64']}:
    raise SystemExit('Unsupported package label')
binary='flight-ui.exe' if label.startswith('windows-') else 'flight-ui'
build=Path('target')/args.target/'release' if args.target else Path('target/release')
if not (build/binary).is_file():
    raise SystemExit(f'Missing executable: {build/binary}')
name=f'prp-flight-{label}'
out=Path('release')
out.mkdir(exist_ok=True)
with tempfile.TemporaryDirectory() as temporary:
    root=Path(temporary)/name
    root.mkdir()
    shutil.copy2(build/binary,root/binary)
    params=root/'public/groth16'
    params.mkdir(parents=True)
    for source in Path('public/groth16').iterdir():
        if source.is_file() and source.suffix in {'.json','.vk'}:
            shutil.copy2(source,params/source.name)
    for folder in ['data','templates']:
        shutil.copytree(Path('public')/folder,root/'public'/folder)
    for file in ['problem.pdf','IMAGE_FORMAT.md']:
        shutil.copy2(Path('public')/file,root/'public'/file)
    for file in ['README.md','DEPLOY_GITHUB.md']:
        shutil.copy2(file,root/file)
    (root/'licenses').mkdir()
    for source in ['assets/OFL.txt','public/THIRD_PARTY_LICENSES.txt']:
        shutil.copy2(source,root/'licenses'/Path(source).name)
    (root/'START_HERE.txt').write_text(
        '打开 flight-ui（Windows 为 flight-ui.exe）。验证证书无需额外参数。\n'
        '首次生成某一问时，程序自动从 GitHub 下载所需参数并缓存；以后无需重复下载。\n'
        '三问首次下载约 229 / 428 / 456 MB；仅校验和验证证书无需下载参数。'
        '下载中断后可重试，已完成的分片会保留。\n'
        '离线使用：可下载 prp-flight-parameters-qN.zip，将其中 public 文件夹合并到程序目录。'
        '完成后应存在 public/groth16/qN-0.dat，不要额外嵌套目录。\n'
        'Linux 需要桌面环境及 X11/Wayland/OpenGL 运行库；macOS / Windows 程序未作开发者签名。\n'
        '无需 Node、Python 或服务器。所有方案处理、证明与验证均在本机完成。\n',encoding='utf-8')
    if system=='windows' or label.startswith('windows-'):
        archive=out/f'{name}.zip'
        with zipfile.ZipFile(archive,'w',compression=zipfile.ZIP_DEFLATED,compresslevel=6) as z:
            for file in sorted(root.rglob('*')):
                if file.is_file():z.write(file,file.relative_to(root.parent))
    else:
        archive=out/f'{name}.tar.gz'
        with tarfile.open(archive,'w:gz') as tar:
            tar.add(root,arcname=name)
print(archive.resolve())
