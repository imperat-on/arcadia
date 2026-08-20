"""Adapters e parsers puros dos providers do Arcadia."""

from .parsers import parse_vdf
from .steam import build_steam_game

__all__ = ["parse_vdf", "build_steam_game"]
