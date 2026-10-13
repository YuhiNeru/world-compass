"""Read-only collection evidence; never reads credentials or session files."""
import json
import pathlib
import sqlite3

root = pathlib.Path(__file__).resolve().parents[1]
progress = json.loads((root / '.runtime/userdata/corpus-progress.json').read_text(encoding='utf8'))
with sqlite3.connect('file:' + str(root / '.runtime/userdata/personal.sqlite') + '?mode=ro', uri=True) as db:
    imported = db.execute('SELECT COUNT(*) FROM worlds').fetchone()[0]
    states = dict(db.execute('SELECT state,COUNT(*) FROM import_queue GROUP BY state'))
    queries = [dict(zip(['category','kind','offset','state'], row)) for row in db.execute('SELECT category,kind,offset,state FROM collection_queries WHERE offset>0 ORDER BY rank,rowid LIMIT 5')]
    images = db.execute('SELECT COUNT(*) FROM images').fetchone()[0]
    translations = db.execute('SELECT COUNT(*) FROM translations').fetchone()[0]
    db.execute('ATTACH DATABASE ? AS previous', (str(root / '.runtime/backups/before-expanded-20261009/personal.sqlite'),))
    preserved = db.execute('SELECT COUNT(*) FROM previous.worlds old JOIN main.worlds current ON old.id=current.id AND old.record=current.record').fetchone()[0]
with sqlite3.connect('file:' + str(root / '.runtime/catalog/catalog.sqlite') + '?mode=ro', uri=True) as db:
    db.execute('ATTACH DATABASE ? AS personal', (str(root / '.runtime/userdata/personal.sqlite'),))
    manifest = json.loads((root / 'resources/setup-manifest.json').read_text(encoding='utf8'))
    version = next(asset['version'] for asset in manifest['assets'] if asset['id'] == 'embedding')
    indexed = db.execute("SELECT COUNT(*) FROM vectors v JOIN personal.import_queue q ON q.id=v.id WHERE q.state='done' AND v.model=?", (version,)).fetchone()[0]
report = dict(version=json.loads((root / 'package.json').read_text(encoding='utf8'))['version'], progress=progress, publicWorlds=imported, indexed=indexed, originalRecordsPreserved=preserved, states=states, searchCursors=queries, cachedImages=images, cachedTranslations=translations)
assert preserved == 1000
assert progress['expanded'] and progress['focus'] == 'japanese-expanded-v1'
(root / 'artifacts/expanded-collection-status.json').write_text(json.dumps(report, ensure_ascii=False, indent=2)+'\n', encoding='utf8')
print(json.dumps(report, ensure_ascii=True))
