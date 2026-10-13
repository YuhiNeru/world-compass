import concurrent.futures, hashlib, json, urllib.request, zipfile
from pathlib import Path
base=Path('artifacts/model-benchmark');base.mkdir(parents=True,exist_ok=True)
meta=Path('docs/research/model-selection')
def asset(repo,file,id):
    m=json.loads((meta/(repo.replace('/','--')+'.json')).read_text(encoding='utf-8'))
    f=next(f for f in m['siblings'] if f['rfilename']==file)
    return dict(id=id,url=f'https://huggingface.co/{repo}/resolve/{m["sha"]}/{file}',size=f['size'],sha256=f['lfs']['sha256'],version=m['sha'],file=file)
assets=[asset('Qwen/Qwen3-1.7B-GGUF','Qwen3-1.7B-Q8_0.gguf','chat-balanced'),asset('Qwen/Qwen3-4B-GGUF','Qwen3-4B-Q4_K_M.gguf','chat-quality'),asset('unsloth/Qwen3.5-4B-GGUF','Qwen3.5-4B-Q4_K_M.gguf','chat-next')]
r=json.loads((meta/'llama-b11497.json').read_text())
a=next(a for a in r['assets'] if a['name']=='llama-b11497-bin-win-vulkan-x64.zip')
assets.append(dict(id='runtime-vulkan',url=a['browser_download_url'],size=a['size'],sha256=a['digest'].split(':')[1],version='b11497',file=a['name']))
(base/'assets.json').write_text(json.dumps(assets,indent=2),encoding='utf-8')
def download(a):
    dest=base/a['file']; part=dest.with_suffix(dest.suffix+'.part')
    if dest.exists() and dest.stat().st_size==a['size'] and hashlib.file_digest(dest.open('rb'),'sha256').hexdigest()==a['sha256']:return dest
    offset=part.stat().st_size if part.exists() else 0
    req=urllib.request.Request(a['url'],headers={'User-Agent':'WorldCompass-model-evaluation/0.2',**({'Range':f'bytes={offset}-'} if offset else {})})
    with urllib.request.urlopen(req,timeout=60) as response:
        if response.status!=206:offset=0
        done=offset;last=-1
        with part.open('ab' if offset else 'wb') as f:
            while b:=response.read(1024*1024):
                f.write(b);done+=len(b);percent=done*100//a['size']//10*10
                if percent!=last:print(a['id'],f'{percent}%',flush=True);last=percent
    if part.stat().st_size!=a['size'] or hashlib.file_digest(part.open('rb'),'sha256').hexdigest()!=a['sha256']:raise RuntimeError('Integrity mismatch: '+a['id'])
    part.replace(dest)
    if dest.suffix=='.zip':
        target=base/'vulkan';target.mkdir(exist_ok=True)
        with zipfile.ZipFile(dest) as z:z.extractall(target)
    return dest
with concurrent.futures.ThreadPoolExecutor(max_workers=3) as pool:
    for p in pool.map(download,assets):print('Verified',p,flush=True)
