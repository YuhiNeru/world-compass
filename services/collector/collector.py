"""Headless public VRChat collector. Python 3.10+, standard library only.

The database contains public metadata and discovery state only. Credentials are
entered interactively and cookies remain in a separate owner-only file.
"""
import argparse
import base64
import contextlib
import datetime
import email.utils
import getpass
import http.cookiejar
import json
import os
from pathlib import Path
import re
import signal
import sqlite3
import sys
import time
import urllib.error
import urllib.parse
import urllib.request

BASE = "https://api.vrchat.cloud/api/1/"
VERSION = "0.1.0"
SCHEMA = """
CREATE TABLE IF NOT EXISTS worlds(id TEXT PRIMARY KEY,record TEXT NOT NULL,fetched INTEGER NOT NULL);
CREATE TABLE IF NOT EXISTS import_queue(id TEXT PRIMARY KEY,state TEXT NOT NULL,priority INTEGER NOT NULL DEFAULT 0);
CREATE TABLE IF NOT EXISTS collection_queries(key TEXT PRIMARY KEY,category TEXT NOT NULL,rank INTEGER NOT NULL,kind TEXT NOT NULL,value TEXT NOT NULL,offset INTEGER NOT NULL DEFAULT 0,state TEXT NOT NULL DEFAULT 'pending');
CREATE TABLE IF NOT EXISTS collection_pages(query_key TEXT NOT NULL,offset INTEGER NOT NULL,fingerprint TEXT NOT NULL,PRIMARY KEY(query_key,offset));
CREATE TABLE IF NOT EXISTS collection_memberships(world_id TEXT NOT NULL,category TEXT NOT NULL,query_key TEXT NOT NULL,PRIMARY KEY(world_id,query_key));
CREATE TABLE IF NOT EXISTS collection_rejections(world_id TEXT PRIMARY KEY,reason TEXT NOT NULL);
CREATE TABLE IF NOT EXISTS collector_meta(key TEXT PRIMARY KEY,value TEXT NOT NULL);
CREATE INDEX IF NOT EXISTS collector_next_query ON collection_queries(state,rank);
CREATE INDEX IF NOT EXISTS collector_next_world ON import_queue(state,priority DESC);
"""
TABLES = {
    "worlds": ("id", "record", "fetched"),
    "import_queue": ("id", "state", "priority"),
    "collection_queries": ("key", "category", "rank", "kind", "value", "offset", "state"),
    "collection_pages": ("query_key", "offset", "fingerprint"),
    "collection_memberships": ("world_id", "category", "query_key"),
    "collection_rejections": ("world_id", "reason"),
}
STOP = False


class AuthRequired(Exception):
    pass


class PageLimit(Exception):
    pass


class NotFound(Exception):
    pass


def valid_id(value, prefix="wrld_"):
    return isinstance(value, str) and re.fullmatch(
        re.escape(prefix) + r"[0-9a-fA-F]{8}(?:-[0-9a-fA-F]{4}){3}-[0-9a-fA-F]{12}", value
    ) is not None


def hints(raw):
    text = " ".join([str(raw.get("name", "")), str(raw.get("description", "")),
                     json.dumps(raw.get("tags", []), ensure_ascii=False)]).lower()
    author = str(raw.get("authorName", raw.get("author", "")))
    kana = lambda s: any("\u3040" <= c <= "\u30ff" for c in s)
    kanji = lambda s: any("\u4e00" <= c <= "\u9fff" for c in s)
    explicit = any(word in text for word in ("日本", "japan", "author_tag_jp", "[jp]", "jpn"))
    return (4 if kana(text) else 1 if kanji(text) else 0) + (3 if explicit else 0) + (2 if kana(author) else 1 if kanji(author) else 0)


def normalize(raw):
    if not isinstance(raw, dict) or not valid_id(raw.get("id")):
        raise ValueError("invalid world ID")
    if raw.get("releaseStatus") != "public":
        raise ValueError("non-public world")
    name = raw.get("name")
    if not isinstance(name, str) or not 1 <= len(name) <= 200:
        raise ValueError("invalid world name")
    platforms = sorted({{"standalonewindows": "pc", "android": "quest"}[p["platform"]]
                        for p in raw.get("unityPackages", []) if isinstance(p, dict)
                        and p.get("platform") in ("standalonewindows", "android")})
    tags = [s[11:] for s in raw.get("tags", []) if isinstance(s, str) and s.startswith("author_tag_")][:64]
    source = "vrchat-api-public"
    thumbnail = None
    for image in (raw.get("thumbnailImageUrl"), raw.get("imageUrl")):
        if isinstance(image, str) and re.fullmatch(
            r"https://api\.vrchat\.cloud/api/1/file/file_[0-9a-fA-F-]{36}/[0-9]+/file", image
        ):
            thumbnail = {"url": image, "license": "VRChat公開画像・個人閲覧用キャッシュ（再配布許諾なし）"}
            break
    capacity = raw.get("capacity")
    return {"schemaVersion": 1, "worldId": raw["id"], "name": name,
            "description": raw["description"][:8000] if isinstance(raw.get("description"), str) else None,
            "author": raw.get("authorName") if isinstance(raw.get("authorName"), str) else None,
            "tags": tags, "platforms": platforms or None,
            "capacity": capacity if type(capacity) is int and capacity > 0 else None,
            "contentWarnings": None, "thumbnail": thumbnail,
            "sourceUrl": "https://vrchat.com/home/world/" + raw["id"],
            "fetchedAt": datetime.datetime.now(datetime.timezone.utc).isoformat(timespec="seconds").replace("+00:00", "Z"),
            "sourceLicense": "VRChat公開API・個人用ローカルキャッシュ（再配布許諾なし）",
            "permissionRef": "personal-local-cache", "deleted": False,
            "attributes": {k: {"value": None, "sourceId": source} for k in ("rain", "night", "chill", "photo", "nature", "game", "light")},
            "sources": [{"id": source, "label": "VRChat公開API（説明・タグは作者の記載。体験と軽さは未確認）",
                         "url": BASE + "worlds/" + raw["id"]}]}


def open_db(path):
    path.parent.mkdir(parents=True, exist_ok=True)
    db = sqlite3.connect(path, timeout=30)
    db.row_factory = sqlite3.Row
    db.execute("PRAGMA journal_mode=WAL")
    db.executescript(SCHEMA)
    return db


def seed(db, plan):
    with db:
        for cat in plan["categories"]:
            for field, kind in (("terms", "search"), ("tags", "tag")):
                for word in cat[field]:
                    db.execute("INSERT OR IGNORE INTO collection_queries(key,category,rank,kind,value) VALUES (?,?,?,?,?)",
                               (f'{cat["id"]}:{kind}:{word}', cat["label"], cat["priority"], kind, word))
        if plan.get("kanaSweep"):
            for code in list(range(0x3041, 0x3097)) + list(range(0x30a1, 0x30fb)):
                word = chr(code)
                db.execute("INSERT OR IGNORE INTO collection_queries(key,category,rank,kind,value) VALUES (?,?,?,?,?)",
                           ("kana:" + word, "日本語文字の補完", 8, "search", word))


def add_creator(db, raw):
    author = raw.get("authorId")
    if hints(raw) > 0 and valid_id(author, "usr_"):
        db.execute("INSERT OR IGNORE INTO collection_queries(key,category,rank,kind,value) VALUES (?,?,?,?,?)",
                   ("creator:" + author, "関連作者の公開作品", 9, "creator", author))


def search_path(q):
    params = {"n": 100, "offset": q["offset"], "sort": "favorites", "order": "descending",
              "releaseStatus": "public", "fuzzy": "false"}
    params[{"creator": "userId", "tag": "tag", "search": "search"}[q["kind"]]] = (
        "author_tag_" + q["value"] if q["kind"] == "tag" else q["value"])
    return "worlds?" + urllib.parse.urlencode(params)


def save_page(db, q, rows):
    if not isinstance(rows, list) or any(not isinstance(w, dict) for w in rows):
        raise ValueError("invalid search response")
    fingerprint = ",".join(sorted({w["id"] for w in rows if isinstance(w.get("id"), str)}))
    repeated = db.execute("SELECT 1 FROM collection_pages WHERE query_key=? AND fingerprint=?", (q["key"], fingerprint)).fetchone()
    forced = q["kind"] == "creator" or q["kind"] == "tag" and q["value"] in ("jp", "japanese", "japan", "jpn")
    with db:
        if not repeated:
            for w in rows:
                if w.get("releaseStatus") != "public" or not valid_id(w.get("id")):
                    continue
                if q["kind"] == "creator" and w.get("authorId") != q["value"]:
                    continue
                if q["kind"] == "tag" and "author_tag_" + q["value"].lower() not in [str(t).lower() for t in w.get("tags", [])]:
                    continue
                hint = hints(w)
                if q["kind"] == "tag" and not forced and not hint:
                    continue
                priority = hint + (20 if forced else 0)
                db.execute("INSERT OR IGNORE INTO collection_memberships VALUES (?,?,?)", (w["id"], q["category"], q["key"]))
                db.execute("INSERT OR IGNORE INTO import_queue VALUES (?,'queued',?)", (w["id"], priority))
                db.execute("UPDATE import_queue SET priority=MAX(priority,?),state=CASE WHEN state='filtered' AND ?>=20 THEN 'queued' ELSE state END WHERE id=?", (priority, priority, w["id"]))
                add_creator(db, w)
            db.execute("INSERT INTO collection_pages VALUES (?,?,?)", (q["key"], q["offset"], fingerprint))
        state = "limited" if repeated else "done" if len(rows) < 100 else "pending"
        db.execute("UPDATE collection_queries SET offset=?,state=? WHERE key=?", (q["offset"] + len(rows), state, q["key"]))


def retry_delay(value, now=None):
    now = time.time() if now is None else now
    try:
        return max(2, int(value))
    except (ValueError, TypeError):
        try:
            return max(2, email.utils.parsedate_to_datetime(value).timestamp() - now)
        except (ValueError, TypeError, AttributeError):
            return 60


def is_page_limit(status, path, body):
    url = urllib.parse.urlsplit(BASE + path)
    offset = urllib.parse.parse_qs(url.query).get("offset", ["0"])[0]
    message = body.get("error", {}).get("message", "") if isinstance(body, dict) else ""
    return status == 403 and url.path == "/api/1/worlds" and offset.isdigit() and int(offset) > 0 and isinstance(message, str) and message.strip().strip('"').lower() == "you've gone too far, pagination stops here."


class NoRedirect(urllib.request.HTTPRedirectHandler):
    def redirect_request(self, *args, **kwargs):
        raise RuntimeError("API redirect refused")


def wait_until(deadline):
    while time.time() < deadline:
        if STOP:
            raise KeyboardInterrupt
        time.sleep(min(0.2, max(0, deadline - time.time())))


class Api:
    def __init__(self, directory, db, contact, interval=2):
        if not isinstance(contact, str) or len(contact) > 200 or any(c.isspace() or ord(c) < 32 for c in contact) or not ("@" in contact or contact.startswith("https://")):
            raise ValueError("Set developer contact in config.json")
        self.db, self.contact, self.interval = db, contact, max(2, interval)
        self.directory = directory
        self.cookie_path = directory / "session.cookies"
        self.cookies = http.cookiejar.MozillaCookieJar(str(self.cookie_path))
        if self.cookie_path.exists():
            if os.name != "nt" and self.cookie_path.stat().st_mode & 0o077:
                raise RuntimeError("Cookie file must have mode 600")
            self.cookies.load(ignore_discard=True, ignore_expires=False)
        self.opener = urllib.request.build_opener(NoRedirect(), urllib.request.HTTPCookieProcessor(self.cookies))

    def request(self, path, body=None, basic=None):
        if path.startswith(("/", "http")) or ".." in path:
            raise ValueError("invalid API path")
        for attempt in range(7):
            row = self.db.execute("SELECT value FROM collector_meta WHERE key='next_request'").fetchone()
            wait_until(float(row[0]) if row else 0)
            with self.db:
                self.db.execute("INSERT OR REPLACE INTO collector_meta VALUES ('next_request',?)", (str(time.time() + self.interval),))
            headers = {"User-Agent": f"WorldCompassCollector/{VERSION} {self.contact}", "Accept": "application/json"}
            if basic:
                pair = ":".join(urllib.parse.quote(v, safe="") for v in basic)
                headers["Authorization"] = "Basic " + base64.b64encode(pair.encode()).decode()
            data = json.dumps(body).encode() if body is not None else None
            if data:
                headers["Content-Type"] = "application/json"
            try:
                response = self.opener.open(urllib.request.Request(BASE + path, data=data, headers=headers), timeout=25)
                status = response.status
            except urllib.error.HTTPError as e:
                response, status = e, e.code
            except (urllib.error.URLError, TimeoutError, OSError):
                if attempt == 6:
                    raise RuntimeError("API unavailable; saved cursor retained") from None
                wait_until(time.time() + min(120, 20 * 2 ** attempt))
                continue
            with response:
                raw = response.read(4 * 1024 * 1024 + 1)
                delay = retry_delay(response.headers.get("Retry-After"))
            self.cookies.save(ignore_discard=True, ignore_expires=False)
            if os.name != "nt":
                os.chmod(self.cookie_path, 0o600)
            if len(raw) > 4 * 1024 * 1024:
                raise RuntimeError("API response too large")
            try:
                parsed = json.loads(raw)
            except (ValueError, UnicodeDecodeError):
                parsed = None
            if is_page_limit(status, path, parsed):
                raise PageLimit
            if status in (401, 403):
                if path.startswith("auth/"):
                    (self.directory / "login-status.json").write_text(json.dumps({"phase": "authentication-error", "httpStatus": status, "secondFactor": "twofactorauth" in path}), encoding="utf-8")
                raise AuthRequired(f"VRChat authentication failed (HTTP {status}); check login username or second-factor code")
            if status == 404:
                raise NotFound
            if status == 400 and path.startswith("worlds?") and int(urllib.parse.parse_qs(urllib.parse.urlsplit(path).query).get("offset", [0])[0]) > 0:
                raise PageLimit
            if status == 429 or status >= 500:
                with self.db:
                    self.db.execute("INSERT OR REPLACE INTO collector_meta VALUES ('next_request',?)", (str(time.time() + (delay if status == 429 else min(120, 20 * 2 ** attempt))),))
                if status == 429:
                    print("API rate limit: waiting before retry", flush=True)
                    continue
                if attempt < 6:
                    continue
            if not 200 <= status < 300 or parsed is None:
                raise RuntimeError(f"API request failed (HTTP {status}); cursor retained")
            return parsed
        raise RuntimeError("API retries exhausted; cursor retained")

    def begin_login(self, username, password):
        # A failed or incomplete login must not reuse an earlier challenge session.
        self.cookies.clear()
        (self.directory / "login-status.json").write_text('{"phase":"awaiting-credentials"}', encoding="utf-8")
        result = self.request("auth/user", basic=(username, password))
        methods = result.get("requiresTwoFactorAuth", [])
        if methods:
            method = "totp" if "totp" in methods else "emailOtp" if "emailOtp" in methods else None
            if method is None:
                raise AuthRequired("Unsupported second factor; use VRChat-supported login method")
            result = {"phase": "second-factor", "method": method}
            (self.directory / "login-status.json").write_text(json.dumps(result), encoding="utf-8")
            return result
        self.accept_login(result)
        return {"phase": "verified"}

    def accept_login(self, result):
        if not valid_id(result.get("id"), "usr_") or result.get("requiresTwoFactorAuth"):
            raise AuthRequired("Login not verified")
        (self.directory / "login-status.json").write_text('{"phase":"verified"}', encoding="utf-8")

    def finish_login(self, code):
        stage = json.loads((self.directory / "login-status.json").read_text(encoding="utf-8"))
        method = stage.get("method")
        if stage.get("phase") != "second-factor" or method not in ("totp", "emailOtp"):
            raise AuthRequired("Start a new login before entering a code")
        if not isinstance(code, str) or not code.isdigit() or not 6 <= len(code) <= 8:
            raise AuthRequired("Enter the current second-factor code")
        response = self.request("auth/twofactorauth/" + method + "/verify", {"code": code})
        if response.get("verified") is not True:
            raise AuthRequired("Second factor not verified")
        self.accept_login(self.request("auth/user"))
        return {"phase": "verified"}

    def login(self):
        username = input("VRChat login username (not display name): ").strip()
        password = getpass.getpass("VRChat password: ")
        result = self.begin_login(username, password)
        password = None
        if result["phase"] == "second-factor":
            method = result["method"]
            code = getpass.getpass(f"VRChat {method} code: ")
            self.finish_login(code)
        print("Login verified. Credentials were not saved; session cookies are owner-only.")


def status(db, directory, stage, message=""):
    counts = dict(db.execute("SELECT state,count(*) FROM import_queue GROUP BY state"))
    queries = dict(db.execute("SELECT state,count(*) FROM collection_queries GROUP BY state"))
    result = {"version": VERSION, "updatedAt": datetime.datetime.now(datetime.timezone.utc).isoformat(),
              "stage": stage, "completed": counts.get("done", 0), "queued": counts.get("queued", 0),
              "skipped": counts.get("skipped", 0), "filtered": counts.get("filtered", 0),
              "queriesDone": queries.get("done", 0) + queries.get("limited", 0),
              "queriesTotal": sum(queries.values()), "limitedQueries": queries.get("limited", 0), "message": message}
    temp = directory / "status.tmp"
    temp.write_text(json.dumps(result, ensure_ascii=False), encoding="utf-8")
    temp.replace(directory / "status.json")
    return result


def work_once(db, api, directory):
    item = db.execute("SELECT id,priority FROM import_queue WHERE state='queued' ORDER BY priority DESC,rowid LIMIT 1").fetchone()
    if item:
        status(db, directory, "fetching")
        try:
            raw = api.request("worlds/" + item["id"])
            if not isinstance(raw, dict) or raw.get("id") != item["id"]:
                raise RuntimeError("API world ID mismatch; cursor retained")
            if hints(raw) == 0 and item["priority"] < 20:
                state, reason = "filtered", "Japanese signal not found in public metadata"
                world = None
            else:
                try:
                    world = normalize(raw)
                    state, reason = "done", None
                except (ValueError, TypeError, KeyError):
                    state, reason, world = "skipped", "Invalid public metadata", None
        except NotFound:
            state, reason, world = "skipped", "Public world not found", None
        with db:
            if world:
                db.execute("INSERT OR IGNORE INTO worlds VALUES (?,?,?)", (item["id"], json.dumps(world, ensure_ascii=False, separators=(",", ":")), int(time.time())))
                add_creator(db, raw)
            if reason:
                db.execute("INSERT OR REPLACE INTO collection_rejections VALUES (?,?)", (item["id"], reason))
            db.execute("UPDATE import_queue SET state=? WHERE id=?", (state, item["id"]))
        status(db, directory, "fetching")
        return True
    q = db.execute("SELECT * FROM collection_queries WHERE state='pending' ORDER BY rank,rowid LIMIT 1").fetchone()
    if not q:
        status(db, directory, "complete", "All scheduled queries processed; API coverage is not exhaustive")
        return False
    status(db, directory, "discovering", q["category"])
    try:
        save_page(db, q, api.request(search_path(q)))
    except PageLimit:
        with db:
            db.execute("UPDATE collection_queries SET state='limited' WHERE key=?", (q["key"],))
    return True


def snapshot(source, output):
    """Export only allowed public tables, using one consistent SQLite read transaction."""
    if source.resolve() == output.resolve():
        raise ValueError("Snapshot destination must differ from source")
    output.parent.mkdir(parents=True, exist_ok=True)
    temp = output.with_suffix(".tmp.sqlite")
    if temp.exists():
        temp.unlink()
    with contextlib.closing(sqlite3.connect(source.resolve().as_uri() + "?mode=ro", uri=True)) as src:
        src.execute("BEGIN")
        names = {r[0] for r in src.execute("SELECT name FROM sqlite_master WHERE type='table'")}
        with contextlib.closing(open_db(temp)) as dst:
            with dst:
                for table, columns in TABLES.items():
                    if table not in names:
                        continue
                    cols = ",".join(columns)
                    rows = src.execute(f"SELECT {cols} FROM {table} ORDER BY rowid")
                    while batch := rows.fetchmany(500):
                        dst.executemany(f"INSERT INTO {table}({cols}) VALUES ({','.join('?' for _ in columns)})", batch)
            dst.execute("PRAGMA wal_checkpoint(TRUNCATE)")
        src.rollback()
    temp.replace(output)


@contextlib.contextmanager
def lock(directory):
    with (directory / "collector.lock").open("a+b") as handle:
        if os.name == "nt":
            import msvcrt
            handle.seek(0); handle.write(b"0"); handle.flush(); handle.seek(0)
            msvcrt.locking(handle.fileno(), msvcrt.LK_NBLCK, 1)
        else:
            import fcntl
            fcntl.flock(handle.fileno(), fcntl.LOCK_EX | fcntl.LOCK_NB)
        yield


def main():
    os.umask(0o077)
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("command", choices=("init", "login", "login-stdin", "run", "status", "snapshot"))
    parser.add_argument("--data-dir", type=Path, default=Path("data"))
    parser.add_argument("--plan", type=Path, default=Path(__file__).with_name("japanese-collection-plan.json"))
    parser.add_argument("--source", type=Path)
    parser.add_argument("--output", type=Path)
    args = parser.parse_args()
    if args.command == "snapshot":
        if not args.source or not args.output:
            parser.error("snapshot requires --source and --output")
        snapshot(args.source, args.output)
        print("Public metadata and discovery state exported; no chats, library, images or credentials included")
        return 0
    directory = args.data_dir.resolve()
    directory.mkdir(parents=True, exist_ok=True)
    if args.command == "status":
        print((directory / "status.json").read_text(encoding="utf-8"))
        return 0
    db = open_db(directory / "collector.sqlite")
    with lock(directory):
        seed(db, json.loads(args.plan.read_text(encoding="utf-8")))
        if args.command == "init":
            print(json.dumps(status(db, directory, "ready")))
            return 0
        config = json.loads((directory / "config.json").read_text(encoding="utf-8"))
        api = Api(directory, db, config.get("contact"), config.get("intervalSeconds", 2))
        if args.command == "login-stdin":
            # Credentials arrive over SSH stdin, never command arguments or files.
            payload = json.loads(sys.stdin.buffer.readline(16385))
            if set(payload) == {"code"}:
                result = api.finish_login(payload["code"])
            elif set(payload) == {"username", "password"} and all(isinstance(v, str) and len(v) <= 4096 for v in payload.values()):
                result = api.begin_login(payload["username"].strip(), payload["password"])
            else:
                raise ValueError("Invalid login input")
            print(json.dumps(result))
            return 0
        if args.command == "login":
            api.login()
            return 0
        user = api.request("auth/user")
        if not valid_id(user.get("id"), "usr_") or user.get("requiresTwoFactorAuth"):
            raise AuthRequired("Interactive login required")
        try:
            while not STOP and work_once(db, api, directory):
                pass
            if STOP:
                status(db, directory, "paused", "Service stopped; saved cursor retained")
        except AuthRequired:
            status(db, directory, "auth-required", "Interactive login required")
            return 20
        except KeyboardInterrupt:
            status(db, directory, "paused", "Service stopped; saved cursor retained")
        except Exception:
            status(db, directory, "error", "Collector stopped; inspect service and retry. Saved cursor retained.")
            raise
    return 0


def stop_signal(*_):
    global STOP
    STOP = True


if __name__ == "__main__":
    signal.signal(signal.SIGTERM, stop_signal)
    signal.signal(signal.SIGINT, stop_signal)
    try:
        sys.exit(main())
    except AuthRequired as error:
        print(str(error) + ". Run the login command in an SSH terminal.", file=sys.stderr)
        sys.exit(20)
    except KeyboardInterrupt:
        sys.exit(0)
    except Exception as error:
        # Never print request headers, bodies, credentials or session objects.
        print(f"Collector stopped ({type(error).__name__}); saved data retained.", file=sys.stderr)
        sys.exit(1)
