"""Parsers locais sem I/O ou dependência do processo Electron."""

from __future__ import annotations

import re
from typing import Any

try:  # python-vdf é opcional; o fallback mantém instalações mínimas funcionais.
    import vdf as _vdf  # type: ignore
except Exception:  # pragma: no cover - depende do ambiente do usuário
    _vdf = None


_KEY_PATTERN = re.compile(r'"((?:[^"\\]|\\.)*)"')


def _parse_fallback(text: str) -> dict[str, Any]:
    root: dict[str, Any] = {}
    stack: list[dict[str, Any]] = [root]
    for raw_line in text.splitlines():
        line = raw_line.strip()
        if not line or line.startswith("//"):
            continue
        if line == "{":
            continue
        if line == "}":
            if len(stack) > 1:
                stack.pop()
            continue
        keys = _KEY_PATTERN.findall(line)
        if len(keys) >= 2:
            stack[-1][keys[0]] = keys[1]
        elif len(keys) == 1:
            child: dict[str, Any] = {}
            stack[-1][keys[0]] = child
            stack.append(child)
    return root


def parse_vdf(text: str) -> dict[str, Any]:
    """Lê VDF via python-vdf quando disponível, com fallback determinístico."""
    if _vdf is not None:
        try:
            parsed = _vdf.loads(text)
            return parsed if isinstance(parsed, dict) else {}
        except Exception:
            # Um ACF parcialmente escrito não deve derrubar todo o provider.
            pass
    return _parse_fallback(text)
