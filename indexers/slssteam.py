"""Parser e transformação puros do bloco AdditionalApps do SLSsteam."""

from __future__ import annotations

import re
from collections.abc import Iterable, Mapping
from typing import Any


_ID = re.compile(r"^(\d+)$")
_KEY = re.compile(r"^(?P<indent>[ \t]*)AdditionalApps\s*:\s*(?P<value>.*)$")
_MAPPING = re.compile(r"^[^:#-][^:]*:\s*(?:.*)$")


def _strip_yaml_comment(value: str) -> str:
    quote = ""
    for index, char in enumerate(value):
        if quote:
            if char == quote and (index == 0 or value[index - 1] != "\\"):
                quote = ""
        elif char in ("'", '"'):
            quote = char
        elif char == "#" and (index == 0 or value[index - 1].isspace()):
            return value[:index].rstrip()
    return value.strip()


def _parse_id(value: str) -> str | None:
    value = _strip_yaml_comment(value).strip().rstrip(",")
    if len(value) >= 2 and value[0] == value[-1] and value[0] in ("'", '"'):
        value = value[1:-1].strip()
    match = _ID.fullmatch(value)
    return match.group(1) if match else None


def _inline_ids(value: str) -> list[str]:
    value = _strip_yaml_comment(value).strip()
    if not (value.startswith("[") and value.endswith("]")):
        return []
    body = value[1:-1]
    # IDs não contêm vírgula; respeitamos aspas para não interpretar um
    # comentário/valor textual como dois itens.
    parts: list[str] = []
    start = 0
    quote = ""
    for index, char in enumerate(body):
        if quote:
            if char == quote and (index == 0 or body[index - 1] != "\\"):
                quote = ""
        elif char in ("'", '"'):
            quote = char
        elif char == ",":
            parts.append(body[start:index])
            start = index + 1
    parts.append(body[start:])
    return [appid for part in parts if (appid := _parse_id(part)) is not None]


def parse_additional_apps(text: str) -> list[str]:
    """Lê listas YAML reais do SLSsteam, incluindo comentários e IDs citados."""
    if not isinstance(text, str):
        return []
    ids: list[str] = []
    in_block = False
    base_indent = -1
    for raw_line in text.splitlines():
        line = raw_line.rstrip("\r")
        match = _KEY.match(line)
        if match:
            in_block = True
            base_indent = len(match.group("indent").expandtabs(2))
            ids.extend(_inline_ids(match.group("value")))
            continue
        if not in_block:
            continue
        stripped = line.lstrip(" \t")
        if not stripped or stripped.startswith("#"):
            continue
        indent = len(line) - len(stripped)
        clean = _strip_yaml_comment(stripped)
        # Um novo mapping no mesmo nível encerra a lista. Não exigimos que
        # esteja sem indentação: isso também cobre blocos YAML aninhados.
        if indent <= base_indent and _MAPPING.match(clean):
            in_block = False
            continue
        if indent <= base_indent:
            in_block = False
            continue
        bullet = re.match(r"^-\s*(.*)$", clean)
        if bullet:
            appid = _parse_id(bullet.group(1))
            if appid is not None:
                ids.append(appid)
    return ids


def _text(value: Any) -> str:
    if value is None:
        return ""
    return str(value).strip()


def _bool(value: Any) -> bool:
    if isinstance(value, str):
        return value.strip().lower() not in {"", "0", "false", "no", "off", "none", "null"}
    return bool(value)


def build_slssteam_game(
    appid: Any,
    existing_appids: Iterable[Any],
    installed: Any,
    art: Mapping[str, Any] | None,
    cdn: Any,
    tool_ids: Iterable[Any],
) -> dict[str, Any] | None:
    appid_text = _text(appid)
    existing = {_text(value) for value in existing_appids}
    tools = {_text(value) for value in tool_ids}
    if not appid_text or appid_text in existing or appid_text in tools:
        return None
    source = art if isinstance(art, Mapping) else {}
    cdn_text = _text(cdn).rstrip("/")
    cover = _text(source.get("cover")) or f"{cdn_text}/{appid_text}/library_600x900.jpg"
    hero = _text(source.get("hero")) or f"{cdn_text}/{appid_text}/library_hero.jpg"
    logo = _text(source.get("logo")) or f"{cdn_text}/{appid_text}/logo.png"
    return {
        "id": f"steam:{appid_text}",
        "title": f"App {appid_text}",
        "launcher": "steam",
        "launch_cmd": ["steam", f"steam://rungameid/{appid_text}"],
        "installed": _bool(installed),
        "cover": cover,
        "hero": hero,
        "logo": logo,
    }
