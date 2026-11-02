"""Exercise the native isolated Groth16 QR image import with a private proof fixture.
Usage: python scripts/test-images.py path/to/verified-certificate.json
Pillow is a development-test dependency only. Test outputs are not shipped.
"""
import base64,io,json,subprocess,sys,time
from pathlib import Path
from PIL import Image,ImageDraw
source=Path(sys.argv[1]);item=json.loads(source.read_text())
folder=Path('.cache/groth16-images');folder.mkdir(exist_ok=True)
original=folder/'original.png'
subprocess.run(['target/release/examples/certificate_io',str(source),str(original)],check=True)
image=Image.open(original).convert('RGB');results=[]
def verify_bytes(name,data,expected=None):
 start=time.monotonic()
 p=subprocess.run(['target/release/flight-ui','--import'],input=json.dumps({'action':'import','file':base64.b64encode(data).decode()}),text=True,capture_output=True,timeout=30,check=True)
 result=json.loads(p.stdout.strip().splitlines()[-1]);ok=result['type']=='result'
 if ok:assert result['result']==item,(name,'wrong recovered claims')
 if expected is not None:assert ok==expected,(name,result)
 row={'case':name,'recovered':ok,'seconds':round(time.monotonic()-start,3),'bytes':len(data)}
 print(json.dumps(row),flush=True);results.append(row);return ok

def run(name,img,ext='PNG',expected=None,**kw):
 data=io.BytesIO();img.save(data,format=ext,**kw)
 path=folder/(name+('.jpg' if ext=='JPEG' else '.png'));path.write_bytes(data.getvalue())
 return verify_bytes(name,data.getvalue(),expected)
run('original',image,expected=True)
for quality in (90,80,70,60):run(f'jpeg{quality}',image,'JPEG',quality=quality,subsampling=2)
for width in (1280,1200,1024,960,800,640):
 smaller=image.resize((width,round(image.height*width/image.width)),Image.Resampling.LANCZOS)
 run(f'resize-{width}',smaller)
 run(f'resize-{width}-jpeg80',smaller,'JPEG',quality=80,subsampling=2)
run('rotated',image.transpose(Image.Transpose.ROTATE_90),expected=True)
changed=image.copy();draw=ImageDraw.Draw(changed);draw.rectangle((500,460,1100,570),fill=(250,249,245));draw.text((550,500),'1 minute - forged visible text',fill='black')
run('changed-text',changed,expected=True) # Actual output must retain the original proven score.
missing=image.copy();ImageDraw.Draw(missing).rectangle((550,820,1050,1190),fill='white')
run('removed-qr',missing,expected=False)
run('grayscale',image.convert('L').convert('RGB'),expected=True)
run('cropped',image.crop((12,12,image.width-12,image.height-12)),expected=True)
run('qr-only',image.crop((550,820,1050,1190)),expected=True)
verify_bytes('pdf-rejected',b'%PDF-1.7\n',False)
if Path('.cache/pdf-vectors/q1.png').exists():verify_bytes('old-jab-rejected',Path('.cache/pdf-vectors/q1.png').read_bytes(),False)
if Path('.cache/formal-tests/previous-ptx1.png').exists():verify_bytes('old-ptx1-rejected',Path('.cache/formal-tests/previous-ptx1.png').read_bytes(),False)
(folder/'results.json').write_text(json.dumps(results,indent=2))
