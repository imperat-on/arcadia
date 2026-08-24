"""Fixtures puras dos primeiros adaptadores de providers."""

import json
import unittest

from pathlib import Path

from indexers.parsers import parse_vdf
from indexers.steam import build_steam_game
from indexers.epic import (
    build_heroic_game,
    build_legendary_game,
    heroic_games_list,
    legendary_games_list,
)
from indexers.lutris import build_lutris_game
from indexers.slssteam import build_slssteam_game, parse_additional_apps


FIXTURES = Path(__file__).resolve().parent / "fixtures" / "providers"


def fixture_json(path: str):
    return json.loads((FIXTURES / path).read_text(encoding="utf-8"))


def fixture_text(path: str) -> str:
    return (FIXTURES / path).read_text(encoding="utf-8")


class IndexerProviderTest(unittest.TestCase):
    def test_parse_vdf_nested_app_state(self):
        parsed = parse_vdf(
            r'''
"AppState"
{
    "appid" "440"
    "name" "Team Fortress 2"
    "installdir" "Team Fortress 2"
}
'''
        )
        self.assertEqual(parsed["AppState"]["appid"], "440")
        self.assertEqual(parsed["AppState"]["name"], "Team Fortress 2")

    def test_steam_game_contract(self):
        game = build_steam_game(
            {"appid": "440", "name": "Team Fortress 2"},
            {"cover": "/cover.jpg", "hero": "/hero.jpg", "logo": "/logo.png"},
        )
        self.assertEqual(game, {
            "id": "steam:440",
            "title": "Team Fortress 2",
            "launcher": "steam",
            "launch_cmd": ["steam", "steam://rungameid/440"],
            "installed": True,
            "cover": "/cover.jpg",
            "hero": "/hero.jpg",
            "logo": "/logo.png",
        })


    def test_legendary_and_heroic_contracts(self):
        legendary = build_legendary_game(
            {
                "app_name": "fortnite",
                "app_title": "Fortnite",
                "metadata": {"keyImages": [
                    {"type": "DieselGameBoxTall", "url": "cover"},
                    {"type": "DieselGameBox", "url": "hero"},
                    {"type": "DieselGameBoxLogo", "url": "logo"},
                ]},
            },
            {"fortnite"},
            Path("/opt/legendary"),
        )
        self.assertEqual(legendary["id"], "epic:fortnite")
        self.assertEqual(legendary["launch_cmd"], ["/opt/legendary", "launch", "fortnite"])
        self.assertTrue(legendary["installed"])
        heroic = build_heroic_game({"appName": "witcher", "title": "Witcher", "is_installed": True}, "gog")
        self.assertEqual(heroic["id"], "heroic:gog:witcher")
        self.assertEqual(heroic["launch_cmd"], ["xdg-open", "heroic://launch/gog/witcher"])
        self.assertEqual(heroic_games_list({"data": [{"appName": "witcher"}]}), [{"appName": "witcher"}])


    def test_lutris_dedup_and_slssteam_block(self):
        self.assertIsNone(build_lutris_game(1, "Steam", "steam", "steam", "440", {"440"}))
        lutris = build_lutris_game(2, "Custom", "custom", "lutris", "", set(), {"cover": "cover"})
        self.assertEqual(lutris["id"], "lutris:2")
        config = "AdditionalApps:\n  - 440\n  - 999\nOther:\n  - 1\n"
        self.assertEqual(parse_additional_apps(config), ["440", "999"])
        sls = build_slssteam_game("999", set(), False, {}, "https://cdn", {"440"})
        self.assertEqual(sls["id"], "steam:999")
        self.assertIsNone(build_slssteam_game("440", set(), False, {}, "https://cdn", {"440"}))

    def test_steam_acf_and_libraryfolders_fixtures_match_golden(self):
        app_manifest = parse_vdf(fixture_text("steam/appmanifest_440.acf"))
        app_state = app_manifest["AppState"]
        self.assertEqual(app_state["InstalledDepots"]["440"]["manifest"], "1234567890123456789")
        libraries = parse_vdf(fixture_text("steam/libraryfolders.vdf"))["libraryfolders"]
        self.assertEqual(libraries["1"]["path"], "/mnt/Games Library/Steam")
        game = build_steam_game(app_state, fixture_json("steam/art.json"))
        self.assertEqual([game], fixture_json("steam/golden.json"))

    def test_legendary_fixture_matches_golden(self):
        games = legendary_games_list(fixture_json("legendary/list-games.json"))
        installed = {
            item["app_name"]
            for item in legendary_games_list(fixture_json("legendary/list-installed.json"))
        }
        actual = [
            game
            for item in games
            if isinstance(item, dict)
            for game in [build_legendary_game(item, installed, Path("/opt/legendary"))]
            if game is not None
        ]
        self.assertEqual(actual, fixture_json("legendary/golden.json"))

    def test_heroic_cache_fixtures_match_golden(self):
        actual = []
        for filename, runner in (("gog_library.json", "gog"), ("legendary_library.json", "legendary")):
            data = fixture_json(f"heroic/{filename}")
            for item in heroic_games_list(data):
                if not isinstance(item, dict):
                    continue
                game = build_heroic_game(item, runner)
                if game is not None:
                    actual.append(game)
        self.assertEqual(actual, fixture_json("heroic/golden.json"))

    def test_lutris_fixture_matches_golden_and_deduplicates_steam(self):
        actual = []
        for item in fixture_json("lutris/games.json"):
            game = build_lutris_game(
                item["id"], item["name"], item["slug"], item["service"],
                item["service_id"], {"440"}, item["art"],
            )
            if game is not None:
                actual.append(game)
        self.assertEqual(actual, fixture_json("lutris/golden.json"))

    def test_slssteam_fixture_matches_golden(self):
        ids = parse_additional_apps(fixture_text("slssteam/config.yaml"))
        self.assertEqual(ids, ["440", "123456", "654321", "1493710", "123456"])
        actual = []
        for appid in dict.fromkeys(ids):
            game = build_slssteam_game(
                appid,
                {"440"},
                False,
                {"cover": "/art/123456.jpg"} if appid == "123456" else {},
                "https://cdn.example/steam/",
                {"1493710"},
            )
            if game is not None:
                actual.append(game)
        self.assertEqual(actual, fixture_json("slssteam/golden.json"))

    def test_steam_tools_are_not_games(self):
        self.assertIsNone(build_steam_game({"appid": "228980", "name": "Steamworks"}))
        self.assertIsNone(build_steam_game({"appid": "999", "name": "Proton Experimental"}))
        self.assertEqual(build_steam_game({"appid": "999", "name": "My Linux Game"})["id"], "steam:999")


if __name__ == "__main__":
    unittest.main()
