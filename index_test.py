"""Testes do envelope e das garantias de escrita do indexador."""

from __future__ import annotations

import importlib.util
import json
import os
import tempfile
import unittest
from pathlib import Path


ROOT = Path(__file__).resolve().parent
INDEX_PATH = ROOT / "index.py"


def load_index(data_dir: Path):
    os.environ["ARCADIA_DATA_DIR"] = str(data_dir)
    name = f"arcadia_index_test_{id(data_dir)}"
    spec = importlib.util.spec_from_file_location(name, INDEX_PATH)
    assert spec and spec.loader
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


class IndexWriterTest(unittest.TestCase):
    def test_future_version_is_preserved(self):
        with tempfile.TemporaryDirectory(prefix="arcadia-index-future-") as raw:
            data_dir = Path(raw)
            target = data_dir / "library.json"
            original = '{"version":99,"games":[{"id":"future","title":"Future"}]}'
            target.write_text(original, encoding="utf-8")
            index = load_index(data_dir)

            self.assertEqual(index.main(), 2)
            self.assertEqual(target.read_text(encoding="utf-8"), original)

    def test_total_provider_failure_preserves_snapshot(self):
        with tempfile.TemporaryDirectory(prefix="arcadia-index-failure-") as raw:
            data_dir = Path(raw)
            target = data_dir / "library.json"
            original = json.dumps({"version": 1, "games": [{"id": "steam:1", "title": "Keep"}]})
            target.write_text(original, encoding="utf-8")
            index = load_index(data_dir)
            index.load_config = lambda: {"sources": {"steam": True, "slssteam": False, "heroic": True, "lutris": True}}
            index.index_steam = lambda: (_ for _ in ()).throw(RuntimeError("steam offline"))
            index.steam_owned_games = lambda _ids: []
            index.enrich_steam = lambda *_args: None
            index.enrich_player = lambda *_args: None
            index.index_epic_and_heroic = lambda: (_ for _ in ()).throw(RuntimeError("heroic offline"))
            index.index_lutris = lambda _ids: (_ for _ in ()).throw(RuntimeError("lutris offline"))

            self.assertEqual(index.main(), 2)
            self.assertEqual(target.read_text(encoding="utf-8"), original)

    def test_success_writes_versioned_document_and_diagnostics(self):
        with tempfile.TemporaryDirectory(prefix="arcadia-index-success-") as raw:
            data_dir = Path(raw)
            index = load_index(data_dir)
            index.load_config = lambda: {"sources": {"steam": True, "slssteam": True, "heroic": True, "lutris": True}}
            index.index_steam = lambda: [{"id": "steam:2", "title": "B", "launcher": "steam", "launch_cmd": ["steam"]}]
            index.steam_owned_games = lambda _ids: []
            index.index_slssteam = lambda *_args: []
            index.enrich_steam = lambda *_args: None
            index.enrich_player = lambda *_args: None
            index.index_epic_and_heroic = lambda: [{"id": "epic:1", "title": "A", "launcher": "heroic", "launch_cmd": ["heroic"]}]
            index.index_lutris = lambda _ids: [{"id": "lutris:3", "title": "C", "launcher": "lutris", "launch_cmd": ["lutris"]}]

            self.assertEqual(index.main(), 0)
            document = json.loads((data_dir / "library.json").read_text(encoding="utf-8"))
            self.assertEqual(document["version"], 1)
            self.assertEqual(document["errors"], [])
            self.assertEqual([game["id"] for game in document["games"]], ["epic:1", "steam:2", "lutris:3"])
            self.assertEqual(document["sources"], {"steam": 1, "heroic": 1, "lutris": 1})


if __name__ == "__main__":
    unittest.main()
