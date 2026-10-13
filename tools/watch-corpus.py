"""Observe public import progress without reading login credentials or session cookies."""
import json, pathlib, sqlite3, time

root = pathlib.Path(__file__).resolve().parents[1]
progress_file = root / '.runtime/userdata/corpus-progress.json'
started = time.monotonic()
last = None
while True:
    try:
        progress = json.loads(progress_file.read_text(encoding='utf8'))
    except (OSError, ValueError):
        time.sleep(5)
        continue
    current = (progress['stage'], progress['completed'], progress['indexed'])
    if current != last:
        print(json.dumps({'stage':current[0], 'worlds':current[1], 'indexed':current[2], 'target':progress['target'], 'elapsedSeconds':round(time.monotonic()-started), 'databaseMB':round(progress['bytes']/1024/1024,2)},ensure_ascii=True),flush=True)
        last = current
    if not progress['running']:
        if progress['stage'] != 'complete':
            print(json.dumps({'paused':True,'message':progress['message']},ensure_ascii=True),flush=True)
            break
        with sqlite3.connect('file:'+str(root/'.runtime/userdata/personal.sqlite')+'?mode=ro',uri=True) as db:
            worlds = [json.loads(row[0]) for row in db.execute('SELECT record FROM worlds')]
            count = db.execute("SELECT COUNT(*) FROM import_queue WHERE state='done'").fetchone()[0]
            image_count = db.execute('SELECT COUNT(*) FROM images').fetchone()[0]
            translation_count = db.execute('SELECT COUNT(*) FROM translations').fetchone()[0]
        with sqlite3.connect('file:'+str(root/'.runtime/catalog/catalog.sqlite')+'?mode=ro',uri=True) as db:
            db.execute('ATTACH DATABASE ? AS personal',(str(root/'.runtime/userdata/personal.sqlite'),))
            indexed = db.execute("SELECT COUNT(*) FROM vectors v JOIN personal.import_queue q ON q.id=v.id WHERE q.state='done'").fetchone()[0]
        assert count >= 1000 and indexed >= 1000 and progress['indexed'] >= 1000
        assert all(w['worldId'].startswith('wrld_') and not w['deleted'] and w['sourceLicense'].startswith('VRChat') for w in worlds)
        report={'version':'0.3.2','completed':True,'publicWorlds':len(worlds),'imported':count,'indexed':indexed,'worldsWithPlatform':sum(bool(w['platforms']) for w in worlds),'worldsWithDescription':sum(bool(w['description']) for w in worlds),'cachedImages':image_count,'cachedTranslations':translation_count,'databaseBytes':sum(p.stat().st_size for p in [root/'.runtime/userdata/personal.sqlite',root/'.runtime/catalog/catalog.sqlite']),'progress':progress}
        (root/'artifacts/corpus-1000-acceptance.json').write_text(json.dumps(report,ensure_ascii=False,indent=2)+'\n',encoding='utf8')
        print(json.dumps(report,ensure_ascii=True),flush=True)
        break
    time.sleep(30)
