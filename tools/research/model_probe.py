import json, urllib.request, urllib.error
from pathlib import Path
out=Path('docs/research/model-selection');out.mkdir(parents=True,exist_ok=True)
def get(url):
    with urllib.request.urlopen(urllib.request.Request(url,headers={'User-Agent':'WorldCompass-development/0.2'}),timeout=40) as r:return json.load(r)
for repo in ['Qwen/Qwen3-1.7B-GGUF','Qwen/Qwen3-4B-GGUF','Qwen/Qwen3-4B-Instruct-2507-GGUF','Qwen/Qwen3.5-2B-GGUF','Qwen/Qwen3.5-4B-GGUF','ggml-org/Qwen3.5-2B-GGUF','ggml-org/Qwen3.5-4B-GGUF']:
    try:
        m=get('https://huggingface.co/api/models/'+repo+'?blobs=true')
        (out/(repo.replace('/','--')+'.json')).write_text(json.dumps(m,ensure_ascii=False,indent=2),encoding='utf-8')
        print(json.dumps({'repo':repo,'sha':m['sha'],'license':m.get('cardData',{}).get('license'),'files':[f for f in m.get('siblings',[]) if f['rfilename'].endswith('.gguf') and any(q in f['rfilename'] for q in ['Q4_K_M','Q5_0','Q8_0'])]},ensure_ascii=False),flush=True)
    except urllib.error.HTTPError as e: print(repo,e.code,flush=True)
r=get('https://api.github.com/repos/ggml-org/llama.cpp/releases/tags/b11497')
(out/'llama-b11497.json').write_text(json.dumps(r,indent=2),encoding='utf-8')
print(json.dumps([{'name':a['name'],'size':a['size'],'digest':a.get('digest'),'url':a['browser_download_url']} for a in r['assets'] if 'win-cuda' in a['name']],indent=2),flush=True)
