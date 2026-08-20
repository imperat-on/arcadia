"""Fixtures puras dos primeiros adaptadores de providers."""

import unittest

from indexers.parsers import parse_vdf
from indexers.steam import build_steam_game


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

    def test_steam_tools_are_not_games(self):
        self.assertIsNone(build_steam_game({"appid": "228980", "name": "Steamworks"}))
        self.assertIsNone(build_steam_game({"appid": "999", "name": "Proton Experimental"}))
        self.assertEqual(build_steam_game({"appid": "999", "name": "My Linux Game"})["id"], "steam:999")


if __name__ == "__main__":
    unittest.main()
