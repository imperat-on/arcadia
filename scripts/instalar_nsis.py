#!/usr/bin/env python3
"""Extrai os dados do NSIS oficial para o NSISDIR do binario Linux.

O pacote `nsis` do apt nao inclui `Include/Win/` nem `Stubs/`, e o template do
Tauri usa um header de `Win/` (RestartManager.nsh). O zip oficial do NSIS tem
esses dados completos, mas so traz `makensis.exe` (Windows).

Uso: instalar_nsis.py <caminho-do-zip> <destino>
"""
import os
import sys
import zipfile


def main() -> int:
    if len(sys.argv) != 3:
        print("uso: instalar_nsis.py <zip> <destino>", file=sys.stderr)
        return 2
    origem, destino = sys.argv[1], sys.argv[2]
    os.makedirs(destino, exist_ok=True)
    with zipfile.ZipFile(origem) as z:
        for nome in z.namelist():
            # O zip tem uma pasta raiz (nsis-3.10/); descarta ela.
            partes = nome.split("/", 1)
            if len(partes) != 2 or not partes[1]:
                continue
            alvo = os.path.join(destino, partes[1])
            if nome.endswith("/"):
                os.makedirs(alvo, exist_ok=True)
                continue
            os.makedirs(os.path.dirname(alvo), exist_ok=True)
            with z.open(nome) as src, open(alvo, "wb") as dst:
                dst.write(src.read())

    obrigatorios = [
        os.path.join(destino, "Include", "Win", "RestartManager.nsh"),
        os.path.join(destino, "Stubs"),
        os.path.join(destino, "Include"),
    ]
    faltando = [caminho for caminho in obrigatorios if not os.path.exists(caminho)]
    if faltando:
        print(f"faltando apos extrair: {faltando}", file=sys.stderr)
        return 1
    print(f"dados do NSIS em {destino}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
