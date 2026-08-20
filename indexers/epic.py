"""Transformações puras para Legendary e caches do Heroic."""

from __future__ import annotations

from collections.abc import Mapping
from pathlib import Path
from typing import Any


def heroic_games_list(data: Any) -> list[Any]:
    if isinstance(data, list):
        return data
    if isinstance(data, dict):
        for key in ("library", "games", "data"):
            if isinstance(data.get(key), list):
                return data[key]
    return []


def _image(game: Mapping[str, Any], *types: str) -> str:
    metadata = game.get("metadata")
    images = metadata.get("keyImages") if isinstance(metadata, Mapping) else []
    for image_type in types:
        for image in images or []:
            if isinstance(image, Mapping) and image.get("type") == image_type and image.get("url"):
                return str(image["url"])
    return ""


def build_legendary_game(game: Mapping[str, Any], installed: set[str], binary: Path) -> dict[str, Any] | None:
    app_name = str(game.get("app_name") or "").strip()
    title = str(game.get("app_title") or "").strip()
    if not app_name or not title:
        return None
    return {
        "id": f"epic:{app_name}",
        "title": title,
        "launcher": "epic",
        "launch_cmd": [str(binary), "launch", app_name],
        "installed": app_name in installed,
        "cover": _image(game, "DieselGameBoxTall", "OfferImageTall"),
        "hero": _image(game, "DieselGameBox", "OfferImageWide", "VaultClosed"),
        "logo": _image(game, "DieselGameBoxLogo"),
    }


def build_heroic_game(game: Mapping[str, Any], runner: str) -> dict[str, Any] | None:
    app_name = str(game.get("app_name") or game.get("appName") or "").strip()
    title = str(game.get("title") or "").strip()
    if not app_name:
        return None
    return {
        "id": f"heroic:{runner}:{app_name}",
        "title": title or app_name,
        "launcher": "heroic",
        "launch_cmd": ["xdg-open", f"heroic://launch/{runner}/{app_name}"],
        "installed": bool(game.get("is_installed")),
        "cover": game.get("art_cover") or game.get("art_square") or "",
        "hero": game.get("art_background") or game.get("art_square") or game.get("art_cover") or "",
        "logo": game.get("art_logo") or "",
    }
