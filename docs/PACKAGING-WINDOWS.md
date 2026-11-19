# Empacotamento Windows (Tauri) a partir do Linux

Gera os dois artefatos do canal Windows sem precisar de uma máquina Windows:

| Artefato | Nome | Uso |
|---|---|---|
| Portátil | `arcadia.exe` + `WebView2Loader.dll` | descompacta e roda, sem instalar |
| Instalador | `Arcadia-<versao>-x64-setup.exe` | NSIS, cria atalhos e entrada em Aplicativos |

```bash
./scripts/package-windows.sh --verify
```

Artefatos em `target/x86_64-pc-windows-gnu/release/` e `…/bundle/nsis/`.

## Por que roda no Linux

O alvo Rust `x86_64-pc-windows-gnu` usa o mingw como linker, e o webview do
Windows é **WebView2** — que já vem no Windows 10/11 atualizado. O Tauri embarca
só o `WebView2Loader.dll`. Resultado: nada de SDK da Microsoft, nada de MSVC.

Diferente do Linux, aqui **não existe problema de codec**: o WebView2 é Chromium
e traz H.264/HLS no próprio binário. O trailer que exigiu injeção de GStreamer
no AppImage funciona nativamente no Windows.

## Ferramentas necessárias

Nenhuma precisa de root. O script aceita um prefixo próprio:

```
ARCADIA_WIN_TOOLS=~/.local/share/arcadia-tools/win
  bin/          x86_64-w64-mingw32-{gcc,ar,dlltool,windres,ld}
  lib/          libs do mingw (kcrt, mingwex, gcc)
  x86_64-w64-mingw32/   headers e CRT
  nsis/         makensis + Stubs + Include + Plugins
```

Na CI, `ARCADIA_WIN_TOOLS=/usr` e as ferramentas vêm do `apt`
(`mingw-w64`, `nsis`).

## Três armadilhas que custaram tempo

**1. `dlltool` faltando.** Sem `mingw-w64-binutils` o build falha dentro do
`windows-sys` com `error calling dlltool ... No such file or directory`. A
mensagem aponta para o crate, não para o pacote ausente.

**2. `stddef.h` do mingw é um wrapper.** O `stddef.h` que vem no pacote do GCC é
só `#include_next <stddef.h>` — o real está em `mingw-w64-headers`. Sem ele o
build do `ring` (usado pelo rustls) morre com `fatal error: stddef.h: No such
file or directory`.

**3. `rc.exe` do Windows SDK.** O `tauri-winres` procura o resource compiler do
toolkit MSVC e aborta com:

    Couldn't execute C:\Program Files (x86)\Windows Kits\10\...\rc.exe

No cross-compile, aponte para o `windres`:

```bash
export RC_x86_64_pc_windows_gnu=x86_64-w64-mingw32-windres
export RC=x86_64-w64-mingw32-windres
```

O script faz isso. **Não** fixe um `RC` com caminho absoluto em
`.cargo/config.toml` — isso quebra todo host que não seja aquela máquina
exata, incluindo a CI.

## O que a verificação prova

`scripts/verify-package-windows.sh` (10 checagens, sem Windows):

- `arcadia.exe` é PE32+ x86-64.
- As DLLs importadas são **só** de sistema (`kernel32`, `user32`, `ole32`,
  `api-ms-win-*`…) mais `WebView2Loader.dll`. Qualquer outra significaria
  runtime ausente na máquina do usuário.
- `WebView2Loader.dll` existe ao lado do exe — sem ela o app não abre.
- O `setup.exe` é um instalador NSIS válido.
- O `installer.nsi` gerado inclui o binário principal, `resources\boot.mp4` e
  `WebView2Loader.dll`.

O payload do NSIS é LZMA sólido, então a checagem lê o script gerado — que é a
fonte da verdade do que entra no pacote — em vez de tentar descomprimir.

## Limites

- **Não assinado.** O SmartScreen vai avisar no setup e no exe. Você decidiu
  abrir mão da assinatura; o aviso é a consequência, não um defeito do pacote.
- **WebView2 ausente.** Windows 10/11 atualizado já tem. Numa instalação limpa
  muito antiga o instalador baixa o bootstrapper da Microsoft — o script NSIS do
  Tauri já contempla esse caminho.
- **Não executado.** Este ambiente é Linux; o `.exe` não roda aqui. Formato,
  arquitetura, dependências e conteúdo do instalador são verificados
  estaticamente. O teste de abertura real só numa máquina Windows.
- **Cross-compile é experimental** do lado do Tauri. Funciona para este app
  (nenhum código Windows-nativo próprio além de `cfg(windows)` do std), mas é
  uma superfície que a Tauri marca como tal.
