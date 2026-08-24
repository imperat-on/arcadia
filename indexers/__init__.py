"""Adapters e parsers puros dos providers do Arcadia."""

from .parsers import parse_vdf
from .steam import build_steam_game
from .epic import (
    build_heroic_game,
    build_legendary_game,
    heroic_games_list,
    legendary_games_list,
)
from .lutris import build_lutris_game
from .slssteam import build_slssteam_game, parse_additional_apps

__all__ = [
    "parse_vdf",
    "build_steam_game",
    "build_heroic_game",
    "build_legendary_game",
    "heroic_games_list",
    "legendary_games_list",
    "build_lutris_game",
    "build_slssteam_game",
    "parse_additional_apps",
]
