"""Merge a trusted SSH-fetched public snapshot into World Compass while it is closed."""
import argparse
import contextlib
import datetime
import json
from pathlib import Path
import sqlite3
import time

from collector import TABLES, valid_id


def merge(source, runtime):
    personal = runtime / "userdata/personal.sqlite"
    if not personal.is_file() or not (runtime / "catalog/catalog.sqlite").is_file():
        raise ValueError("Existing World Compass runtime required")
    if source.resolve() == personal.resolve():
        raise ValueError("Remote snapshot must differ from local database")
    with contextlib.closing(sqlite3.connect(source.resolve().as_uri() + "?mode=ro", uri=True)) as remote:
        # Validate public rows before any local changes. Snapshot is obtained over pinned SSH.
        for world_id, record in remote.execute("SELECT id,record FROM worlds"):
            world = json.loads(record)
            if not valid_id(world_id) or world.get("worldId") != world_id or world.get("permissionRef") != "personal-local-cache" or world.get("schemaVersion") != 1:
                raise ValueError("Invalid public world record")
            if not isinstance(world.get("name"), str) or not 1 <= len(world["name"]) <= 200:
                raise ValueError("Invalid public world name")
        with contextlib.closing(sqlite3.connect(personal, timeout=30)) as local:
            backup_dir = runtime / "backups" / "before-vps-sync"
            backup_dir.mkdir(parents=True, exist_ok=True)
            backup = backup_dir / f"personal-{time.time_ns()}.sqlite"
            with contextlib.closing(sqlite3.connect(backup)) as target:
                local.backup(target)
            old = local.execute("SELECT count(*) FROM worlds").fetchone()[0]
            with local:
                for table, columns in TABLES.items():
                    cols = ",".join(columns)
                    cursor = remote.execute(f"SELECT {cols} FROM {table} ORDER BY rowid")
                    command = "INSERT OR IGNORE" if table in ("worlds", "import_queue") else "INSERT OR REPLACE"
                    while batch := cursor.fetchmany(500):
                        local.executemany(f"{command} INTO {table}({cols}) VALUES ({','.join('?' for _ in columns)})", batch)
                # Existing local records/vectors and all private tables are retained.
                local.execute("UPDATE import_queue SET state='done' WHERE id IN (SELECT id FROM worlds)")
            new = local.execute("SELECT count(*) FROM worlds").fetchone()[0]
        marker = runtime / "userdata/remote-collector.json"
        value = {"mode": "remote", "syncedAt": datetime.datetime.now(datetime.timezone.utc).isoformat(), "worlds": new}
        marker.with_suffix(".tmp").write_text(json.dumps(value), encoding="utf-8")
        marker.with_suffix(".tmp").replace(marker)
        progress = runtime / "userdata/corpus-progress.json"
        p = json.loads(progress.read_text(encoding="utf-8"))
        p.update({"remote": True, "running": False, "automaticResume": True, "completed": new,
                  "stage": "ready", "message": "VPSから同期しました。意味検索を準備します"})
        progress.with_suffix(".tmp").write_text(json.dumps(p, ensure_ascii=False), encoding="utf-8")
        progress.with_suffix(".tmp").replace(progress)
    return {"previousWorlds": old, "worlds": new, "added": new - old, "backup": str(backup)}


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--source", type=Path, required=True)
    parser.add_argument("--runtime-dir", type=Path, required=True)
    args = parser.parse_args()
    print(json.dumps(merge(args.source, args.runtime_dir)))
