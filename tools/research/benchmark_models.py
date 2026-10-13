import json,subprocess,time,urllib.request,socket,uuid,statistics,sys
from pathlib import Path
base=Path('artifacts/model-benchmark')
exe=base/'vulkan/llama-server.exe'
schema=json.loads(Path('packages/contracts/conditions.schema.json').read_text(encoding='utf-8'))
prompt=Path('packages/prompts/conditions-v1.txt').read_text(encoding='utf-8')
cases=[('雨の夜に2人でゆっくりできる場所',{'people':2,'platform':None,'preferred':['rain','night','chill']}),('Quest対応が必須、ゲームなしで4人で写真を撮りたい',{'people':4,'platform':'quest','excluded':['game'],'preferred':['photo']}),('静かな森で一人でぼーっとしたい',{'people':None,'platform':None,'preferred':['nature','chill']}),('人数は6人、雨音は絶対必要。軽さは未確認でいい',{'people':6,'platform':None,'required':['rain']})]
models=[('0.6B-Q8',next(Path('.runtime/models').glob('chat-*/model.gguf'))),('1.7B-Q8',base/'Qwen3-1.7B-Q8_0.gguf'),('4B-Q4_K_M',base/'Qwen3-4B-Q4_K_M.gguf')]
if '--next' in sys.argv: models=[('Qwen3.5-4B-Q4_K_M',base/'Qwen3.5-4B-Q4_K_M.gguf')]
results=[]
for label,model in models:
 key=uuid.uuid4().hex
 with socket.socket() as sock:sock.bind(('127.0.0.1',0));port=sock.getsockname()[1]
 log=(base/(label+'.log')).open('w',encoding='utf-8')
 start=time.monotonic()
 p=subprocess.Popen([str(exe),'-m',str(model),'--host','127.0.0.1','--port',str(port),'--api-key',key,'--ctx-size','4096','--threads','4','--parallel','1','--n-gpu-layers','99','--no-webui'],stdout=log,stderr=log,creationflags=subprocess.CREATE_NO_WINDOW)
 def post(body):
  req=urllib.request.Request(f'http://127.0.0.1:{port}/v1/chat/completions',data=json.dumps(body,ensure_ascii=False).encode(),headers={'Authorization':'Bearer '+key,'Content-Type':'application/json'})
  with urllib.request.urlopen(req,timeout=120) as r:return json.load(r)
 try:
  for _ in range(160):
   if p.poll() is not None:raise RuntimeError('model failed; inspect '+str(log.name))
   try:
    with urllib.request.urlopen(urllib.request.Request(f'http://127.0.0.1:{port}/health',headers={'Authorization':'Bearer '+key}),timeout=1) as r:
     if r.status==200:break
   except Exception:pass
   time.sleep(.25)
  cold=time.monotonic()-start;items=[]
  for text,expected in cases:
   start=time.monotonic()
   r=post({'messages':[{'role':'system','content':prompt},{'role':'user','content':text}],'temperature':0,'max_tokens':256,'chat_template_kwargs':{'enable_thinking':False},'reasoning_effort':'none','response_format':{'type':'json_schema','json_schema':{'name':'conditions','strict':True,'schema':schema}}})
   val=json.loads(r['choices'][0]['message']['content'])
   ok=all(val.get(k)==v if not isinstance(v,list) else set(v).issubset(val.get(k,[])) for k,v in expected.items())
   items.append(dict(query=text,output=val,correct=ok,seconds=round(time.monotonic()-start,3),timings=r.get('timings')))
   print(label,items[-1],flush=True)
  start=time.monotonic()
  r=post({'messages':[{'role':'system','content':'あなたは日本語のワールド探索アシスタントです。2文で短く自然に返答し、最後に希望を1つだけ質問してください。実在のワールド名や場所の事実は作らないでください。'},{'role':'user','content':'今日はちょっと疲れた。どんなところがいいかな？'}],'temperature':.4,'max_tokens':160,'chat_template_kwargs':{'enable_thinking':False},'reasoning_effort':'none'})
  item=dict(model=label,coldSeconds=round(cold,3),passCount=sum(i['correct'] for i in items),cases=items,conversation=r['choices'][0]['message']['content'],conversationSeconds=round(time.monotonic()-start,3),conversationTimings=r.get('timings'))
  results.append(item)
  (base/('results-next.json' if '--next' in sys.argv else 'results.json')).write_text(json.dumps(results,ensure_ascii=False,indent=2),encoding='utf-8')
  print(label,'PASS',item['passCount'],'cold',cold,'conversation',item['conversation'],flush=True)
 finally:p.terminate();p.wait(timeout=10);log.close()
