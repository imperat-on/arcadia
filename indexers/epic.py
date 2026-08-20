"""Transformações puras para Legendary e caches do Heroic."""

from __future__ import annotations

from collections.abc import Iterable, Mapping
from pathlib import Path
from typing import Any


def _text(value: Any) -> str:
    if value is None:
        return ""
    return str(value).strip()


def _bool(value: Any) -> bool:
    if isinstance(value, str):
        return value.strip().lower() not in {"", "0", "false", "no", "off", "none", "null"}
    return bool(value)


def _first_text(game: Mapping[str, Any], *keys: str) -> str:
    for key in keys:
        value = _text(game.get(key))
        if value:
            return value
    return ""


def _games_list(data: Any) -> list[Any]:
    if isinstance(data, list):
        return data
    if isinstance(data, Mapping):
        for key in ("library", "games", "data"):
            value = data.get(key)
            if isinstance(value, list):
                return value
    return []


def heroic_games_list(data: Any) -> list[Any]:
    """Extrai entradas dos formatos de cache conhecidos do Heroic."""
    return _games_list(data)


def legendary_games_list(data: Any) -> list[Any]:
    """Extrai a lista de ``list-games --json`` do Legendary.

    Legendary normalmente imprime uma lista, mas versões que envelopam a
    resposta em ``games``/``data`` também são encontradas em instalações
    antigas e wrappers de distribuição.
    """
    return _games_list(data)


def _image(game: Mapping[str, Any], *types: str) -> str:
    metadata = game.get("metadata")
    images: Any = metadata.get("keyImages") if isinstance(metadata, Mapping) else None
    if not isinstance(images, list):
        images = game.get("keyImages", [])
    wanted = {image_type.lower() for image_type in types}
    for image in images:
        if not isinstance(image, Mapping):
            continue
        image_type = _text(image.get("type")).lower()
        url = _text(image.get("url"))
        if image_type in wanted and url:
            return url
    return ""


def _art(game: Mapping[str, Any], *keys: str) -> str:
    for key in keys:
        value = _text(game.get(key))
        if value:
            return value
    return ""


def build_legendary_game(
    game: Mapping[str, Any],
    installed: Iterable[Any],
    binary: Path,
) -> dict[str, Any] | None:
    """Converte uma entrada do Legendary sem executar o runner."""
    app_name = _first_text(game, "app_name", "appName")
    title = _first_text(game, "app_title", "title", "name")
    if not app_name or not title:
        return None
    installed_ids = {_text(value) for value in installed}
    binary_text = _text(binary)
    return {
        "id": f"epic:{app_name}",
        "title": title,
        "launcher": "epic",
        "launch_cmd": [binary_text, "launch", app_name],
        "installed": app_name in installed_ids,
        "cover": _image(game, "DieselGameBoxTall", "OfferImageTall"),
        "hero": _image(game, "DieselGameBox", "OfferImageWide", "VaultClosed"),
        "logo": _image(game, "DieselGameBoxLogo"),
    }


def build_heroic_game(game: Mapping[str, Any], runner: Any) -> dict[str, Any] | None:
    """Converte uma entrada de ``*_library.json`` do Heroic."""
    app_name = _first_text(game, "app_name", "appName", "app_id", "appId")
    if not app_name:
        return None
    runner_name = _text(runner).lower() or "unknown"
    title = _first_text(game, "title", "app_title", "name") or app_name
    cover = _art(game, "art_cover", "artCover", "cover", "cover_url")
    square = _art(game, "art_square", "artSquare", "square")
    hero = _art(game, "art_background", "artBackground", "background", "hero")
    logo = _art(game, "art_logo", "artLogo", "logo")
    return {
        "id": f"heroic:{runner_name}:{app_name}",
        "title": title,
        "launcher": "heroic",
        "launch_cmd": ["xdg-open", f"heroic://launch/{runner_name}/{app_name}"],
        "installed": _bool(game.get("is_installed", game.get("installed"))),
        "cover": cover or square,
        "hero": hero or square or cover,
        "logo": logo,
    }
