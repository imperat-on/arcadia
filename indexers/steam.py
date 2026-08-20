"""Transformação pura de um AppState Steam em ArcadiaGame."""

from __future__ import annotations

from collections.abc import Mapping
from typing import Any

DEFAULT_STEAM_TOOL_IDS = frozenset({
    "228980", "1493710", "1070560", "1391110", "1628350", "1826330", "2180100",
})
DEFAULT_STEAM_TOOL_WORDS = ("runtime", "redistributable", "proton", "steamworks")


def build_steam_game(
    app_state: Mapping[str, Any],
    art: Mapping[str, str] | None = None,
    *,
    tool_ids=DEFAULT_STEAM_TOOL_IDS,
    tool_words=DEFAULT_STEAM_TOOL_WORDS,
) -> dict[str, Any] | None:
    """Converte ACF já parseado; não acessa disco, rede ou configuração."""
    appid = str(app_state.get("appid", "")).strip()
    title = str(app_state.get("name", "")).strip()
    if not appid or appid in tool_ids:
        return None
    if any(word in title.lower() for word in tool_words):
        return None
    assets = dict(art or {})
    return {
        "id": f"steam:{appid}",
        "title": title or f"App {appid}",
        "launcher": "steam",
        "launch_cmd": ["steam", f"steam://rungameid/{appid}"],
        "installed": True,
        "cover": assets.get("cover", ""),
        "hero": assets.get("hero", ""),
        "logo": assets.get("logo", ""),
    }
