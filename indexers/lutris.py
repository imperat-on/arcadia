"""Transformação pura de registros Lutris."""

from __future__ import annotations

from collections.abc import Iterable, Mapping
from typing import Any


def _text(value: Any) -> str:
    if value is None:
        return ""
    return str(value).strip()


def build_lutris_game(
    gid: Any,
    name: Any,
    slug: Any,
    service: Any,
    service_id: Any,
    steam_appids: Iterable[Any] | None = None,
    art: Mapping[str, Any] | None = None,
) -> dict[str, Any] | None:
    """Normaliza uma linha instalada da base ``pga.db`` do Lutris."""
    game_id = _text(gid)
    if not game_id:
        return None
    safe_slug = _text(slug)
    title = _text(name) or safe_slug or f"Game {game_id}"
    service_name = _text(service).lower()
    service_game_id = _text(service_id)
    steam_ids = {_text(value) for value in (steam_appids or ())}
    if service_name == "steam" and service_game_id in steam_ids:
        return None
    source = art if isinstance(art, Mapping) else {}
    assets = {key: _text(source.get(key)) for key in ("cover", "hero", "logo")}
    return {
        "id": f"lutris:{game_id}",
        "title": title,
        "launcher": "lutris",
        "launch_cmd": ["lutris", f"lutris:rungameid/{game_id}"],
        "installed": True,
        "cover": assets["cover"],
        "hero": assets["hero"],
        "logo": assets["logo"],
    }
