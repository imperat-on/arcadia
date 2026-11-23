# Empacotamento do Arcadia no Linux e Windows

O `arcadia-native` roda as interfaces Desktop e Big Picture em Rust com iced.
Os scripts geram um tarball Linux, um AppImage Linux e um zip Windows.

## Build e execução

```bash
cargo run -p arcadia-native
```

Check e testes rápidos (dentro de worktrees, use o wrapper de
[NATIVE-UI-WORKERS.md](NATIVE-UI-WORKERS.md), que limita jobs e timeout):

```bash
cargo check -p arcadia-native
cargo test -p arcadia-native
```

Para testar a verificação de fontes com desafio JavaScript, compile também o
helper; o app procura `arcadia-source-helper` ao lado do executável:

```bash
cargo build -p arcadia-native -p arcadia-source-helper
```

O app restaura a sessão salva na pasta de dados. Sem sessão, abre com a
biblioteca local. Use `ARCADIA_DATA_DIR` para selecionar uma pasta diferente.

As fontes Inter são embutidas no binário via `include_bytes!`. O `boot.mp4` não
é embutido: é procurado ao lado do executável, em `resources/boot.mp4` e no data
dir. Para rodar do checkout, o `boot.mp4` da raiz já atende.

## Dependências de build

Além do toolchain Rust estável:

| Dependência | Para quê | Debian/Ubuntu |
|---|---|---|
| GStreamer 1.0 core | `arcadia-native-media` | `libgstreamer1.0-dev` |
| GStreamer app/video | `appsink`, caps de vídeo | `libgstreamer-plugins-base1.0-dev` |
| `libudev` | gamepad (`gilrs`) | `libudev-dev` |
| GTK 3 e WebKitGTK 4.1 | helper isolado de verificação de fontes (`arcadia-source-helper`); a UI principal não usa webview | `libgtk-3-dev`, `libwebkit2gtk-4.1-dev` |
| dav1d 1.3 ou mais novo | decodificação de capas AVIF | `libdav1d-dev` |
| `pkg-config` | descoberta das libs acima | `pkg-config` |

A interface principal usa iced/wgpu. No Linux, o binário também abre uma
janela GTK/WebKitGTK em outro processo quando uma fonte exige verificação
interativa. O player toca vídeo com a saída de áudio desativada.

### Windows

| Dependência | Para quê |
|---|---|
| Rust estável **MSVC** (`x86_64-pc-windows-msvc`) | binário nativo |
| Visual Studio Build Tools 2022 + Windows SDK | linker MSVC e headers do Windows |
| GStreamer **1.28.9 MSVC x86_64** (runtime + devel) | `arcadia-native-media` e o mesmo runtime que vai no zip |
| `pkg-config` (vem no SDK GStreamer; fallback Chocolatey `pkgconfiglite`) | descoberta de `gstreamer-1.0.pc` |
| dav1d 1.3 ou mais novo | decodificação de capas AVIF |

O GStreamer do Windows não vem de um gerenciador de pacotes: use o instalador
oficial. A versão pinada é a **1.28.9** porque o `gstreamer-rs 0.25.4` acompanha
as APIs 1.28 e o runtime embarcado precisa casar com o SDK usado no build.
A partir da 1.28 o instalador é Inno Setup e traz runtime e arquivos de
desenvolvimento no mesmo `.exe`:

```powershell
# PowerShell elevado ou usuário com permissão no diretório escolhido.
# /TYPE=devel instala runtime + headers + .lib + .pc (o SDK de fato).
.\gstreamer-1.0-msvc-x86_64-1.28.9.exe /VERYSILENT /NORESTART `
  /TYPE=devel /DIR=C:\gstreamer
```

Depois, na mesma sessão (ou em "Variáveis de Ambiente"):

```powershell
$env:GSTREAMER_ROOT_X86_64 = "C:\gstreamer"
$env:PKG_CONFIG_PATH = "C:\gstreamer\lib\pkgconfig"
$env:PATH = "C:\gstreamer\bin;$env:PATH"
```

Para dav1d, a CI usa `vcpkg install dav1d:x64-windows-static-md` e acrescenta
`installed/x64-windows-static-md/lib/pkgconfig` do vcpkg a `PKG_CONFIG_PATH`.

O CI faz exatamente isso, mas baixa o instalador e confere o SHA-256 antes de
executar. Versões 1.28.x do mesmo ABI são compatíveis entre si; não misture
runtime MinGW (`libgstreamer-1.0-0.dll`) com um binário MSVC.

## Dependências de runtime

O tarball Linux não embarca GStreamer. A distribuição do usuário precisa
fornecer:

| Uso | Debian/Ubuntu | Fedora | Arch |
|---|---|---|---|
| pipeline base | `gstreamer1.0-plugins-base` | `gstreamer1-plugins-base` | `gst-plugins-base` |
| HTTP/MP4 | `gstreamer1.0-plugins-good` | `gstreamer1-plugins-good` | `gst-plugins-good` |
| HLS/H.264 | `gstreamer1.0-plugins-bad` | `gstreamer1-plugins-bad-free` | `gst-plugins-bad` |
| alternativa H.264 | `gstreamer1.0-libav` | `gstreamer1-libav` (RPM Fusion) | `gst-libav` |

Também são necessários:

- driver gráfico com Vulkan ou OpenGL e sessão X11/Wayland (wgpu);
- bibliotecas `libudev`, `libdav1d`, GTK 3 e WebKitGTK 4.1;
- rede apenas para capas, trailers e recursos online.

Fontes do sistema não são obrigatórias para a interface: a Inter vai embutida.

### AppImage Linux

O AppImage (`scripts/package-native-appimage.sh`) embute o GStreamer do host
de build — core, plugins e codecs — em `usr/lib/arcadia-gst`. O `AppRun` isola
`GST_PLUGIN_SYSTEM_PATH_1_0` e `GST_PLUGIN_SCANNER`, então o player usa sempre a
mesma versão, independente da distro do usuário. O script valida o bundle com
`gst-inspect-1.0` e toca um MP4 real antes de gerar a imagem.

GTK/WebKit **não** entram no AppImage: são usados apenas pelo
`arcadia-source-helper`, que fica em `usr/bin` e roda em processo separado.
Sem GTK/WebKit no host, a UI principal e os vídeos continuam funcionando; só a
verificação de fonte com desafio JavaScript fica indisponível.

O binário principal não carrega GTK/WebKit no startup: a UI é iced/wgpu e o
helper só é spawnado quando uma fonte responde com desafio Cloudflare.

### Windows

O zip pode levar o runtime GStreamer embutido (quando empacotado com
`GSTREAMER_ROOT_*`) ou usar o GStreamer instalado na máquina. Em ambos os casos
o runtime precisa ser **MSVC x86_64 1.28** e é necessário o **Microsoft Visual
C++ Redistributable 2015–2022 x64** para as DLLs do GStreamer e o binário MSVC.

O launcher `run-arcadia-native.cmd` configura para o runtime embutido:

| Variável | Valor |
|---|---|
| `PATH` | `gstreamer\bin;%PATH%` |
| `GST_PLUGIN_PATH` / `GST_PLUGIN_PATH_1_0` | `gstreamer\lib\gstreamer-1.0` |
| `GST_PLUGIN_SYSTEM_PATH_1_0` | `gstreamer\lib\gstreamer-1.0` (isolado do host) |
| `GST_PLUGIN_SCANNER` | `gstreamer\libexec\gstreamer-1.0\gst-plugin-scanner.exe` |

Sem o bundle, instale o runtime oficial e exporte as mesmas variáveis apontando
para `C:\gstreamer\1.0\msvc_x86_64` (ou para o diretório escolhido no
instalador). `arcadia-native.exe` sozinho não acha as DLLs em `gstreamer\bin`.

## Empacotar no Linux

```bash
./scripts/package-native-linux.sh
./scripts/package-native-appimage.sh
```

O primeiro script roda `cargo build --locked --release -p arcadia-native -p arcadia-gamepad -p arcadia-source-helper`
(respeitando `CARGO_BUILD_JOBS` e `CARGO_TARGET_DIR`) e gera, em
`dist/native-linux/`:

```text
arcadia-native-<versao>-linux-<arch>.tar.gz
SHA256SUMS
```

O segundo gera o AppImage com GStreamer embutido em
`dist/native-linux/appimage/`:

```text
arcadia-native-<versao>-linux-<arch>.AppImage
SHA256SUMS
```

Dentro do tarball, tudo em uma pasta `arcadia-native-<versao>-linux-<arch>/`:

| Arquivo | Papel |
|---|---|
| `arcadia-native` | binário release |
| `arcadia-gamepad` | auxiliar de controle para jogos no Linux |
| `arcadia-source-helper` | janela isolada de verificação de fontes (GTK/WebKit; opcional em runtime) |
| `boot.mp4` | tela de abertura (se existir no checkout) |
| `arcadia-native.desktop` | atalho X11/Wayland |
| `arcadia-native.png` | ícone 512×512 |
| `resources/torrent_rpc/` | worker Python usado para torrents |
| `README.md` | dependências, instalação e limitações |

O ícone do aplicativo vem de `crates/arcadia-native/assets/icons/icon.png`.

### Flags e variáveis

```bash
./scripts/package-native-linux.sh --skip-build          # usa binário já compilado
./scripts/package-native-linux.sh --output /tmp/arcadia # muda o diretório de saída
ARCADIA_NATIVE_BIN=/caminho/arcadia-native \
  ./scripts/package-native-linux.sh --skip-build
```

`--skip-build` procura `arcadia-native` e `arcadia-gamepad` em `target/release/`
ou no diretório de compilação escolhido em `$CARGO_TARGET_DIR`. Para indicar
arquivos já compilados, use `ARCADIA_NATIVE_BIN` e `ARCADIA_GAMEPAD_BIN`.

O tarball é determinístico para o mesmo conteúdo: entradas ordenadas, `mtime`,
owner e group fixos e `gzip -n` sem nome/timestamp. Rodar duas vezes com o mesmo
binário gera o mesmo SHA-256.

### Instalar o tarball

Para rodar diretamente, extraia o tarball e execute `./arcadia-native` na
pasta extraída. Para adicionar o aplicativo ao menu:

```bash
tar -xzf arcadia-native-<versao>-linux-<arch>.tar.gz
cd arcadia-native-<versao>-linux-<arch>
mkdir -p ~/.local/bin ~/.local/share/applications \
         ~/.local/share/icons/hicolor/512x512/apps ~/.local/bin/resources/torrent_rpc
install -m 0755 arcadia-native arcadia-gamepad ~/.local/bin/
install -m 0644 boot.mp4 ~/.local/bin/
install -m 0644 resources/torrent_rpc/*.py ~/.local/bin/resources/torrent_rpc/
install -m 0644 arcadia-native.desktop ~/.local/share/applications/
install -m 0644 arcadia-native.png ~/.local/share/icons/hicolor/512x512/apps/
```

O `boot.mp4` fica ao lado do binário em `~/.local/bin`, que é onde o app o
procura pelo `current_exe()`. Sem ele, a interface abre direto.

Para verificar o download, rode `sha256sum -c SHA256SUMS` na pasta onde o
tarball e o checksum foram baixados.

Em um checkout do código-fonte, `./install.sh` compila o binário e instala
o aplicativo para o usuário atual.

## Empacotar no Windows

```powershell
pwsh -File .\scripts\package-native-windows.ps1
```

Se só houver Windows PowerShell 5.1, use
`powershell -ExecutionPolicy Bypass -File .\scripts\package-native-windows.ps1`.

O script roda `cargo build --release -p arcadia-native` e gera, em
`dist/native-windows/`:

```text
arcadia-native-<versao>-windows-x86_64.zip
SHA256SUMS
```

Dentro do zip, tudo em `arcadia-native-<versao>-windows-x86_64/`:

| Arquivo | Papel |
|---|---|
| `arcadia-native.exe` | binário release |
| `boot.mp4` | tela de abertura (se existir no checkout) |
| `resources/torrent_rpc/` | arquivos do worker de torrent |
| `run-arcadia-native.cmd` | launcher com as variáveis do GStreamer (só com runtime embarcado) |
| `gstreamer\bin\*.dll` | runtime GStreamer (somente DLLs) |
| `gstreamer\lib\gstreamer-1.0\*.dll` | plugins (base, good, bad, libav etc.) |
| `gstreamer\libexec\gstreamer-1.0\gst-plugin-scanner.exe` | scanner de plugins (quando presente) |
| `README.md` | dependências, execução e limitações |

O `gstreamer\` só existe quando `-GStreamerRoot` ou `GSTREAMER_ROOT_X86_64` /
`GSTREAMER_ROOT` / `GSTREAMER_1_0_ROOT_MSVC_X86_64` aponta para um runtime
válido (checado por `bin\gstreamer-1.0-0.dll` e `lib\gstreamer-1.0`). Só o
runtime é copiado — nunca headers, `.lib` ou o SDK inteiro. Sem GStreamer o zip
ainda é gerado, com aviso e um README que instrui a instalar o runtime; o
binário e o código Rust não mudam.

### Flags e variáveis (Windows)

```powershell
.\scripts\package-native-windows.ps1 -SkipBuild
.\scripts\package-native-windows.ps1 -Output C:\tmp\arcadia
.\scripts\package-native-windows.ps1 -GStreamerRoot C:\gstreamer
$env:ARCADIA_NATIVE_BIN = "C:\build\arcadia-native.exe"
.\scripts\package-native-windows.ps1 -SkipBuild
```

`-SkipBuild` procura `target\release\arcadia-native.exe` (ou
`$env:CARGO_TARGET_DIR\release\arcadia-native.exe`). Se o `.exe` não existir, o
script falha com o comando de build; não copia `target\` inteiro nem
`Cargo.lock`. `dist/` continua no `.gitignore`.

O nome do zip é determinístico e as entradas usam ordenação e timestamp fixo
(2020-01-01), então o mesmo conteúdo gera o mesmo SHA-256, independente do
relógio do build.

### Rodar e verificar

Extraia o zip e execute `run-arcadia-native.cmd`. O `boot.mp4` fica ao lado do
`.exe` (o app também procura `resources\boot.mp4`). O checksum fica fora do zip:

```powershell
Get-FileHash -Algorithm SHA256 .\arcadia-native-<versao>-windows-x86_64.zip
Get-Content .\SHA256SUMS
```

## CI

O job `native` em `.github/workflows/ci.yml` roda em `ubuntu-24.04`:

1. instala as dependências Linux de build e os plugins GStreamer de runtime;
2. roda os testes do workspace;
3. roda `./scripts/package-native-linux.sh` (build release + tarball);
4. publica o `.tar.gz` e o `SHA256SUMS` como artifact `arcadia-native-linux`.

O job `native-windows` roda em `windows-latest`:

1. baixa `gstreamer-1.0-msvc-x86_64-1.28.9.exe` do site oficial, confere o
   SHA-256 e instala com `/TYPE=devel /CURRENTUSER` em
   `%RUNNER_TEMP%\gstreamer`;
2. usa o `pkg-config` do SDK GStreamer, com `pkgconfiglite` como alternativa,
   e instala dav1d com vcpkg;
3. exporta `GSTREAMER_ROOT_X86_64`, `PKG_CONFIG_PATH` e o `bin` no `PATH`;
4. roda `cargo check -p arcadia-native`;
5. roda `scripts/package-native-windows.ps1` e publica o `.zip` e o
   `SHA256SUMS` como artifact `arcadia-native-windows`.

## Limitações conhecidas

- **Atualização dos arquivos portáteis.** Para atualizar o tarball ou zip,
  baixe e extraia o novo pacote.
- **Windows sem assinatura.** O zip não é assinado; o SmartScreen pode avisar
  na primeira execução e não há atalho no menu Iniciar.
- **GStreamer embutido é opcional no Windows.** Se `GSTREAMER_ROOT_*` não
  estiver definido, o zip sai sem `gstreamer\` e exige GStreamer 1.28 MSVC
  x86_64 + VC++ 2015–2022 na máquina. Use `run-arcadia-native.cmd` quando o
  runtime for embarcado: o `.exe` sozinho não procura DLLs em `gstreamer\bin`.
- **Zip grande com GStreamer.** Copiar o runtime inteiro (`bin` + plugins)
  deixa o `.zip` na casa das centenas de MB; sem GStreamer ele fica pequeno.
- **No Linux, GStreamer vem do sistema.** Instale os plugins da distribuição.
  Se `hlsdemux` ou o decoder H.264 estiver ausente, o player reporta a falha.
- **glibc do host de build.** O binário exige a glibc de quem compilou. O
  artefato da CI usa Ubuntu 24.04 (glibc 2.39). Sistemas mais antigos podem
  precisar compilar a partir do código-fonte.
