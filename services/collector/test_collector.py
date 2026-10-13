import contextlib
import json
from pathlib import Path
import sqlite3
import shutil
import uuid
import unittest

from collector import (Api, AuthRequired, PageLimit, TABLES, hints, is_page_limit,
                       normalize, open_db, retry_delay, save_page, snapshot,
                       work_once)
from sync_local import merge

WORLD = "wrld_4432ea9b-729c-46e3-8eaf-846aa0a37fdd"
AUTHOR = "usr_4432ea9b-729c-46e3-8eaf-846aa0a37fdd"


class FakeApi:
    def __init__(self, response):
        self.response = response

    def request(self, path):
        if isinstance(self.response, Exception):
            raise self.response
        return self.response


class Tests(unittest.TestCase):
    def setUp(self):
        test_root = Path(__file__).resolve().parents[2] / ".runtime" / "collector-tests"
        test_root.mkdir(parents=True, exist_ok=True)
        self.directory = test_root / uuid.uuid4().hex
        self.directory.mkdir()
        self.db = open_db(self.directory / "collector.sqlite")

    def tearDown(self):
        self.db.close()
        expected = Path(__file__).resolve().parents[2] / ".runtime" / "collector-tests"
        assert self.directory.resolve().parent == expected.resolve()
        shutil.rmtree(self.directory)

    def query(self, kind="search", value="チル"):
        self.db.execute("INSERT INTO collection_queries(key,category,rank,kind,value) VALUES ('test','test',1,?,?)", (kind, value))
        self.db.commit()
        return self.db.execute("SELECT * FROM collection_queries").fetchone()

    def world(self, **changes):
        return dict({"id": WORLD, "releaseStatus": "public", "name": "チル部屋", "authorId": AUTHOR}, **changes)

    def test_pagination_and_existing_ids_are_preserved(self):
        q = self.query()
        self.db.execute("INSERT INTO import_queue VALUES (?,'done',0)", (WORLD,))
        self.db.commit()
        save_page(self.db, q, [self.world() for _ in range(100)])
        q = self.db.execute("SELECT * FROM collection_queries WHERE key='test'").fetchone()
        self.assertEqual(q["offset"], 100)
        self.assertEqual(self.db.execute("SELECT state FROM import_queue WHERE id=?", (WORLD,)).fetchone()[0], "done")
        save_page(self.db, q, [self.world() for _ in range(100)])
        self.assertEqual(self.db.execute("SELECT state FROM collection_queries WHERE key='test'").fetchone()[0], "limited")

    def test_creator_filter_does_not_trust_wrong_author(self):
        q = self.query("creator", AUTHOR)
        save_page(self.db, q, [self.world(authorId="usr_00000000-0000-0000-0000-000000000000")])
        self.assertEqual(self.db.execute("SELECT count(*) FROM import_queue").fetchone()[0], 0)

    def test_japanese_tag_requires_matching_api_tag(self):
        q = self.query("tag", "jp")
        save_page(self.db, q, [self.world(tags=["author_tag_english"])])
        self.assertEqual(self.db.execute("SELECT count(*) FROM import_queue").fetchone()[0], 0)

    def test_api_attributes_are_not_inferred_from_description(self):
        world = normalize(self.world(description="rain chill night", unityPackages=[{"platform": "android"}]))
        self.assertEqual(world["platforms"], ["quest"])
        self.assertTrue(all(v["value"] is None for v in world["attributes"].values()))
        self.assertGreater(hints(self.world()), 0)

    def test_invalid_metadata_is_skipped_and_persisted(self):
        self.db.execute("INSERT INTO import_queue VALUES (?,'queued',20)", (WORLD,))
        self.db.commit()
        self.assertTrue(work_once(self.db, FakeApi(self.world(name="")), self.directory))
        self.assertEqual(self.db.execute("SELECT state FROM import_queue").fetchone()[0], "skipped")
        self.assertEqual(self.db.execute("SELECT count(*) FROM worlds").fetchone()[0], 0)
        self.assertEqual(self.db.execute("SELECT count(*) FROM collection_rejections").fetchone()[0], 1)

    def test_authentication_failure_keeps_pending_world(self):
        self.db.execute("INSERT INTO import_queue VALUES (?,'queued',20)", (WORLD,))
        self.db.commit()
        with self.assertRaises(AuthRequired):
            work_once(self.db, FakeApi(AuthRequired()), self.directory)
        self.assertEqual(self.db.execute("SELECT state FROM import_queue").fetchone()[0], "queued")

    def test_page_limit_moves_to_next_query(self):
        self.query()
        work_once(self.db, FakeApi(PageLimit()), self.directory)
        self.assertEqual(self.db.execute("SELECT state FROM collection_queries").fetchone()[0], "limited")

    def test_exact_pagination_error_only(self):
        body = {"error": {"message": '"you\'ve gone too far, pagination stops here."'}}
        self.assertTrue(is_page_limit(403, "worlds?offset=1000", body))
        for status, path in [(401, "worlds?offset=1000"), (403, "auth/user?offset=1000"), (403, "worlds?offset=0")]:
            self.assertFalse(is_page_limit(status, path, body))
        self.assertFalse(is_page_limit(403, "worlds?offset=1000", {"error": {"message": "Missing Credentials"}}))

    def test_public_snapshot_excludes_private_tables_and_keeps_cursors(self):
        self.query()
        self.db.execute("UPDATE collection_queries SET offset=600 WHERE key='test'")
        self.db.execute("CREATE TABLE chats(secret TEXT)")
        self.db.execute("INSERT INTO chats VALUES ('private')")
        self.db.execute("INSERT INTO collector_meta VALUES ('sensitive','do not export')")
        self.db.commit()
        output = self.directory / "public.sqlite"
        snapshot(self.directory / "collector.sqlite", output)
        with contextlib.closing(sqlite3.connect(output)) as exported:
            names = {r[0] for r in exported.execute("SELECT name FROM sqlite_master")}
            self.assertNotIn("chats", names)
            self.assertEqual(exported.execute("SELECT count(*) FROM collector_meta").fetchone()[0], 0)
            self.assertEqual(exported.execute("SELECT offset FROM collection_queries WHERE key='test'").fetchone()[0], 600)

    def test_retry_after_http_date(self):
        self.assertEqual(retry_delay("30", 0), 30)
        self.assertEqual(retry_delay("Thu, 01 Jan 1970 00:01:00 GMT", 0), 60)
        self.assertEqual(retry_delay("invalid", 0), 60)

    def test_gui_auth_returns_only_public_phase_and_requires_verification(self):
        api = Api(self.directory, self.db, "test@example.com")
        responses = iter([{"requiresTwoFactorAuth": ["totp"]}, {"verified": True}, {"id": AUTHOR}])
        api.request = lambda *args, **kwargs: next(responses)
        phase = api.begin_login("test-user", "test-password")
        self.assertEqual(phase, {"phase": "second-factor", "method": "totp"})
        with self.assertRaises(AuthRequired):
            api.finish_login("bad-code")
        self.assertEqual(api.finish_login("123456"), {"phase": "verified"})
        text = (self.directory / "login-status.json").read_text(encoding="utf-8")
        self.assertNotIn("test-user", text)
        self.assertNotIn("test-password", text)
        self.assertEqual(json.loads(text), {"phase": "verified"})

    def test_sync_preserves_private_data_and_existing_records(self):
        runtime = self.directory / "runtime"
        (runtime / "catalog").mkdir(parents=True)
        with contextlib.closing(sqlite3.connect(runtime / "catalog/catalog.sqlite")) as catalog:
            catalog.execute("CREATE TABLE vectors(id TEXT,vector TEXT)")
            catalog.execute("INSERT INTO vectors VALUES (?, 'unchanged')", (WORLD,))
            catalog.commit()
        local = open_db(runtime / "userdata/personal.sqlite")
        local.executescript("CREATE TABLE chats(record TEXT); INSERT INTO chats VALUES ('private');")
        local.execute("INSERT INTO worlds VALUES (?,?,1)", (WORLD, json.dumps(normalize(self.world()))))
        local.commit()
        old_record = local.execute("SELECT record FROM worlds").fetchone()[0]
        local.close()
        (runtime / "userdata/corpus-progress.json").write_text('{}', encoding='utf-8')
        second = "wrld_00000000-0000-0000-0000-000000000001"
        self.db.execute("INSERT INTO worlds VALUES (?,?,2)", (WORLD, json.dumps(normalize(self.world(name="changed")))))
        self.db.execute("INSERT INTO worlds VALUES (?,?,2)", (second, json.dumps(normalize(self.world(id=second)))))
        self.db.commit()
        result = merge(self.directory / "collector.sqlite", runtime)
        self.assertEqual(result["added"], 1)
        with contextlib.closing(sqlite3.connect(runtime / "userdata/personal.sqlite")) as local:
            self.assertEqual(local.execute("SELECT record FROM worlds WHERE id=?", (WORLD,)).fetchone()[0], old_record)
            self.assertEqual(local.execute("SELECT record FROM chats").fetchone()[0], "private")
        with contextlib.closing(sqlite3.connect(runtime / "catalog/catalog.sqlite")) as catalog:
            self.assertEqual(catalog.execute("SELECT vector FROM vectors").fetchone()[0], "unchanged")
        self.assertTrue((runtime / "userdata/remote-collector.json").is_file())


if __name__ == "__main__":
    unittest.main()
