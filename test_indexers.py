"""Fixtures puras dos primeiros adaptadores de providers."""

import unittest

from pathlib import Path

from indexers.parsers import parse_vdf
from indexers.steam import build_steam_game
from indexers.epic import build_heroic_game, build_legendary_game, heroic_games_list
from indexers.lutris import build_lutris_game
from indexers.slssteam import build_slssteam_game, parse_additional_apps


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

    def test_steam_tools_are_not_games(self):
        self.assertIsNone(build_steam_game({"appid": "228980", "name": "Steamworks"}))
        self.assertIsNone(build_steam_game({"appid": "999", "name": "Proton Experimental"}))
        self.assertEqual(build_steam_game({"appid": "999", "name": "My Linux Game"})["id"], "steam:999")


if __name__ == "__main__":
    unittest.main()
