"""Parsers locais sem I/O ou dependência do processo Electron."""

from __future__ import annotations

from typing import Any, Iterator

try:  # python-vdf é opcional; o fallback mantém instalações mínimas funcionais.
    import vdf as _vdf  # type: ignore
except Exception:  # pragma: no cover - depende do ambiente do usuário
    _vdf = None


def _vdf_tokens(text: str) -> Iterator[str]:
    """Tokeniza a variante KeyValues usada pelos arquivos da Steam.

    A implementação anterior dependia de cada par chave/valor ocupar uma linha.
    Isso não é uma garantia do formato: arquivos reais misturam chaves com
    ``{`` na mesma linha, comentários no fim da linha e valores escapados.
    Tokenizar primeiro mantém o fallback pequeno, mas cobre ACF e
    ``libraryfolders.vdf`` sem depender de ``python-vdf``.
    """
    i = 0
    size = len(text)
    while i < size:
        char = text[i]
        if char.isspace():
            i += 1
            continue
        if char == "\ufeff":  # BOM que aparece em cópias editadas no Windows.
            i += 1
            continue
        if char == "/" and i + 1 < size and text[i + 1] == "/":
            end = text.find("\n", i + 2)
            i = size if end < 0 else end + 1
            continue
        if char in "{}":
            yield char
            i += 1
            continue
        if char == '"':
            i += 1
            value: list[str] = []
            while i < size:
                char = text[i]
                if char == '"':
                    i += 1
                    break
                if char == "\\" and i + 1 < size:
                    nxt = text[i + 1]
                    # KeyValues escapa aspas e barras. Para outras sequências
                    # preservamos a barra, que pode ser parte de um caminho.
                    if nxt in ('"', "\\"):
                        value.append(nxt)
                    else:
                        value.extend(("\\", nxt))
                    i += 2
                    continue
                value.append(char)
                i += 1
            yield "".join(value)
            continue

        # Tokens sem aspas são aceitos por alguns exportadores de VDF. Eles
        # terminam em whitespace, chave ou fechamento de bloco.
        start = i
        while i < size and not text[i].isspace() and text[i] not in "{}":
            if text[i] == "/" and i + 1 < size and text[i + 1] == "/":
                break
            i += 1
        if i > start:
            yield text[start:i]
        else:  # evita loop em entrada malformada
            i += 1


def _parse_fallback(text: str) -> dict[str, Any]:
    root: dict[str, Any] = {}
    stack: list[dict[str, Any]] = [root]
    tokens = list(_vdf_tokens(text))
    index = 0
    while index < len(tokens):
        token = tokens[index]
        if token == "}":
            if len(stack) > 1:
                stack.pop()
            index += 1
            continue
        if token == "{":  # bloco sem chave: inválido, mas ignore com segurança.
            index += 1
            continue
        key = token
        index += 1
        if index >= len(tokens):
            break
        value = tokens[index]
        if value == "{":
            child: dict[str, Any] = {}
            stack[-1][key] = child
            stack.append(child)
            index += 1
        elif value == "}":
            # Entrada truncada: não invente valor e deixe o fechamento ser
            # processado na próxima iteração.
            continue
        else:
            stack[-1][key] = value
            index += 1
    return root


def parse_vdf(text: str) -> dict[str, Any]:
    """Lê VDF via python-vdf quando disponível, com fallback determinístico.

    Steam pode deixar um ACF parcialmente escrito durante uma atualização. Em
    vez de propagar erro para os outros providers, retornamos a parte válida do
    documento e deixamos o chamador decidir se há um jogo utilizável.
    """
    if not isinstance(text, str):
        return {}
    if _vdf is not None:
        try:
            parsed = _vdf.loads(text)
            return parsed if isinstance(parsed, dict) else {}
        except Exception:
            # Um ACF parcialmente escrito não deve derrubar todo o provider.
            pass
    return _parse_fallback(text)
