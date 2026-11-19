#!/usr/bin/env python3
"""Gera os `latest*.yml` do auto-update a partir dos artefatos do release.

O updater le `latest-linux.yml` (AppImage) e `latest.yml` (NSIS). O `sha512`
tem que ser do arquivo publicado — o cliente verifica antes de gravar
qualquer byte, e um hash errado aborta a atualizacao.

Uso: gerar_manifest.py <diretorio-dos-artefatos> <versao>
"""
import base64
import hashlib
import os
import sys
from datetime import datetime, timezone


def sha512_base64(caminho: str) -> str:
    digest = hashlib.sha512()
    with open(caminho, "rb") as arquivo:
        for bloco in iter(lambda: arquivo.read(1024 * 1024), b""):
            digest.update(bloco)
    return base64.b64encode(digest.digest()).decode()


def manifest(versao: str, arquivo: str, caminho: str) -> str:
    tamanho = os.path.getsize(caminho)
    sha = sha512_base64(caminho)
    data = datetime.now(timezone.utc).strftime("%Y-%m-%dT%H:%M:%S.000Z")
    return (
        f"version: {versao}\n"
        f"files:\n"
        f"  - url: {arquivo}\n"
        f"    sha512: {sha}\n"
        f"    size: {tamanho}\n"
        f"path: {arquivo}\n"
        f"sha512: {sha}\n"
        f"releaseDate: '{data}'\n"
    )


def main() -> int:
    if len(sys.argv) != 3:
        print("uso: gerar_manifest.py <dir> <versao>", file=sys.stderr)
        return 2
    diretorio, versao = sys.argv[1], sys.argv[2]

    alvos = {
        "latest-linux.yml": f"Arcadia_{versao}_amd64.AppImage",
        "latest.yml": f"Arcadia_{versao}_x64-setup.exe",
    }
    for nome, artefato in alvos.items():
        caminho = os.path.join(diretorio, artefato)
        if not os.path.isfile(caminho):
            print(f"erro: {caminho} nao existe", file=sys.stderr)
            return 1
        saida = os.path.join(diretorio, nome)
        with open(saida, "w", encoding="utf-8") as arquivo:
            arquivo.write(manifest(versao, artefato, caminho))
        print(f"{nome}: {artefato} ({os.path.getsize(caminho)} bytes)")
    return 0


if __name__ == "__main__":
    sys.exit(main())
