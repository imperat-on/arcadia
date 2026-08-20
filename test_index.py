"""Testes do envelope e das garantias de escrita do indexador."""

from __future__ import annotations

import importlib.util
import json
import os
import tempfile
import unittest
from types import SimpleNamespace
from unittest.mock import patch
from pathlib import Path


ROOT = Path(__file__).resolve().parent
INDEX_PATH = ROOT / "index.py"
FIXTURES = ROOT / "fixtures" / "providers"


def load_index(data_dir: Path):
    os.environ["ARCADIA_DATA_DIR"] = str(data_dir)
    name = f"arcadia_index_test_{id(data_dir)}"
    spec = importlib.util.spec_from_file_location(name, INDEX_PATH)
    assert spec and spec.loader
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


class IndexProviderIntegrationTest(unittest.TestCase):
    def test_legendary_command_accepts_wrapped_json_fixture(self):
        with tempfile.TemporaryDirectory(prefix="arcadia-index-legendary-") as raw:
            index = load_index(Path(raw))
            binary = Path(raw) / "legendary"
            binary.touch()
            index.LEGENDARY_BIN = binary
            responses = [
                SimpleNamespace(
                    returncode=0,
                    stdout=(FIXTURES / "legendary/list-games.json").read_text(encoding="utf-8"),
                ),
                SimpleNamespace(
                    returncode=0,
                    stdout=(FIXTURES / "legendary/list-installed.json").read_text(encoding="utf-8"),
                ),
            ]
            with patch("subprocess.run", side_effect=responses):
                games = index.index_legendary()
            self.assertEqual([game["id"] for game in games], ["epic:shadow_tactics", "epic:celeste"])
            self.assertTrue(games[0]["installed"])
            self.assertFalse(games[1]["installed"])

    def test_heroic_cache_fixture_is_read_by_index_provider(self):
        with tempfile.TemporaryDirectory(prefix="arcadia-index-heroic-") as raw:
            index = load_index(Path(raw))
            index.HEROIC_CACHE = FIXTURES / "heroic"
            games = index.index_heroic()
            self.assertEqual(
                [game["id"] for game in games],
                [
                    "heroic:gog:cyberpunk_2077",
                    "heroic:gog:disco_elysium",
                    "heroic:legendary:hades",
                ],
            )


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
