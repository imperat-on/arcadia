"""Transformação pura de um AppState Steam em ArcadiaGame."""

from __future__ import annotations

from collections.abc import Mapping
from typing import Any, Iterable

DEFAULT_STEAM_TOOL_IDS = frozenset({
    "228980", "1493710", "1070560", "1391110", "1628350", "1826330", "2180100",
})
DEFAULT_STEAM_TOOL_WORDS = ("runtime", "redistributable", "proton", "steamworks")


def _text(value: Any) -> str:
    """Converte valores vindos de VDF/JSON em texto de contrato."""
    if value is None:
        return ""
    return str(value).strip()


def _assets(art: Mapping[str, Any] | None) -> dict[str, str]:
    source = art if isinstance(art, Mapping) else {}
    return {name: _text(source.get(name)) for name in ("cover", "hero", "logo")}


def build_steam_game(
    app_state: Mapping[str, Any],
    art: Mapping[str, Any] | None = None,
    *,
    tool_ids: Iterable[Any] = DEFAULT_STEAM_TOOL_IDS,
    tool_words: Iterable[Any] = DEFAULT_STEAM_TOOL_WORDS,
) -> dict[str, Any] | None:
    """Converte ACF já parseado; não acessa disco, rede ou configuração.

    Os arquivos ACF são texto, mas alguns callers (e caches antigos) entregam
    ``appid`` como inteiro ou campos de arte como ``null``. Normalizamos esses
    valores na borda para que o envelope da biblioteca tenha sempre strings.
    """
    appid = _text(app_state.get("appid"))
    title = _text(app_state.get("name"))
    normalized_tool_ids = {_text(value) for value in tool_ids}
    if not appid or appid in normalized_tool_ids:
        return None
    normalized_tool_words = tuple(
        word for word in (_text(value).lower() for value in tool_words) if word
    )
    if any(word in title.lower() for word in normalized_tool_words):
        return None
    assets = _assets(art)
    return {
        "id": f"steam:{appid}",
        "title": title or f"App {appid}",
        "launcher": "steam",
        "launch_cmd": ["steam", f"steam://rungameid/{appid}"],
        "installed": True,
        "cover": assets["cover"],
        "hero": assets["hero"],
        "logo": assets["logo"],
    }
