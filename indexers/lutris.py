"""Transformação pura de registros Lutris."""

from __future__ import annotations

from collections.abc import Mapping, Set
from typing import Any


def build_lutris_game(
    gid: Any,
    name: Any,
    slug: Any,
    service: Any,
    service_id: Any,
    steam_appids: Set[str] | None = None,
    art: Mapping[str, str] | None = None,
) -> dict[str, Any] | None:
    if service == "steam" and str(service_id) in (steam_appids or set()):
        return None
    safe_slug = str(slug or "")
    assets = dict(art or {})
    return {
        "id": f"lutris:{gid}",
        "title": name or safe_slug,
        "launcher": "lutris",
        "launch_cmd": ["lutris", f"lutris:rungameid/{gid}"],
        "installed": True,
        "cover": assets.get("cover", ""),
        "hero": assets.get("hero", ""),
        "logo": assets.get("logo", ""),
    }
