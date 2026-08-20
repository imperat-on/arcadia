"""Parser e transformação puros do bloco AdditionalApps do SLSsteam."""

from __future__ import annotations

import re
from collections.abc import Iterable, Set
from typing import Any


def parse_additional_apps(text: str) -> list[str]:
    ids: list[str] = []
    in_block = False
    for raw_line in text.splitlines():
        line = raw_line.strip("\n")
        if re.match(r"^AdditionalApps\s*:", line):
            in_block = True
            continue
        if in_block:
            if line and not line[0].isspace() and ":" in line and not line.lstrip().startswith("#"):
                break
            match = re.match(r"^\s*-\s*(\d+)", line)
            if match:
                ids.append(match.group(1))
    return ids


def build_slssteam_game(
    appid: str,
    existing_appids: Set[str],
    installed: bool,
    art: dict[str, str] | None,
    cdn: str,
    tool_ids: Iterable[str],
) -> dict[str, Any] | None:
    if appid in existing_appids or appid in set(tool_ids):
        return None
    assets = art or {}
    return {
        "id": f"steam:{appid}",
        "title": f"App {appid}",
        "launcher": "steam",
        "launch_cmd": ["steam", f"steam://rungameid/{appid}"],
        "installed": installed,
        "cover": assets.get("cover") or f"{cdn}/{appid}/library_600x900.jpg",
        "hero": assets.get("hero") or f"{cdn}/{appid}/library_hero.jpg",
        "logo": assets.get("logo") or f"{cdn}/{appid}/logo.png",
    }
