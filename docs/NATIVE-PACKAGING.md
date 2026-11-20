# Empacotamento do frontend nativo (Linux e Windows)

Como buildar, rodar e empacotar o `arcadia-native` — o frontend em Rust + iced,
sem Tauri e sem webview. O pacote Tauri (`scripts/package-linux.sh`,
`scripts/package-windows.sh`, `.deb`/AppImage/RPM/NSIS) é outro caminho e não é
afetado por este documento; veja [docs/PACKAGING-LINUX.md](PACKAGING-LINUX.md)
e [docs/PACKAGING-WINDOWS.md](PACKAGING-WINDOWS.md).

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

O app lê o data dir de `arcadia_store::paths::resolve_data_dir()` e, quando
`ApiClient::from_env()` encontra sessão, restaura a conta como o bootstrap
Tauri. Sem sessão, abre com a biblioteca local/guest.

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
| `pkg-config` | descoberta das libs acima | `pkg-config` |

O nativo **não usa GTK, WebKit nem ALSA**: a janela é iced/wgpu, não há webview
e o áudio do player é um `fakesink`. Essas dependências continuam necessárias
apenas para o app Tauri.

### Windows

| Dependência | Para quê |
|---|---|
| Rust estável **MSVC** (`x86_64-pc-windows-msvc`) | binário nativo |
| Visual Studio Build Tools 2022 + Windows SDK | linker MSVC e headers do Windows |
| GStreamer **1.28.9 MSVC x86_64** (runtime + devel) | `arcadia-native-media` e o mesmo runtime que vai no zip |
| `pkg-config` (vem no SDK GStreamer; fallback Chocolatey `pkgconfiglite`) | descoberta de `gstreamer-1.0.pc` |

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
- `libudev` para gamepad — opcional, presente na maioria das distros;
- rede apenas para capas, trailers e recursos online.

Fontes do sistema não são obrigatórias para a interface: a Inter vai embutida.

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
```

O script roda `cargo build --release -p arcadia-native` (respeitando
`CARGO_BUILD_JOBS` e `CARGO_TARGET_DIR`) e gera, em `dist/native-linux/`:

```text
arcadia-native-<versao>-linux-<arch>.tar.gz
SHA256SUMS
```

Dentro do tarball, tudo em uma pasta `arcadia-native-<versao>-linux-<arch>/`:

| Arquivo | Papel |
|---|---|
| `arcadia-native` | binário release |
| `boot.mp4` | tela de abertura (se existir no checkout) |
| `arcadia-native.desktop` | atalho X11/Wayland |
| `arcadia-native.png` | ícone 512×512 |
| `README.md` | dependências, instalação e limitações |

O ícone vem de `src-tauri/icons/icon.png` (lido, nunca alterado): os PNGs de
`crates/arcadia-native/assets/icons` são glifos da UI, não um ícone de
aplicativo.

### Flags e variáveis

```bash
./scripts/package-native-linux.sh --skip-build          # usa binário já compilado
./scripts/package-native-linux.sh --output /tmp/arcadia # muda o diretório de saída
ARCADIA_NATIVE_BIN=/caminho/arcadia-native \
  ./scripts/package-native-linux.sh --skip-build
```

`--skip-build` procura `target/release/arcadia-native` (ou
`$CARGO_TARGET_DIR/release/arcadia-native`). Se o binário não existir, o script
falha com a instrução de build; não copia `target/` inteiro nem `Cargo.lock`.

O tarball é determinístico para o mesmo conteúdo: entradas ordenadas, `mtime`,
owner e group fixos e `gzip -n` sem nome/timestamp. Rodar duas vezes com o mesmo
binário gera o mesmo SHA-256.

### Instalar o tarball

```bash
tar -xzf arcadia-native-<versao>-linux-<arch>.tar.gz
cd arcadia-native-<versao>-linux-<arch>
mkdir -p ~/.local/bin ~/.local/share/applications \
         ~/.local/share/icons/hicolor/512x512/apps
install -m 0755 arcadia-native boot.mp4 ~/.local/bin/
install -m 0644 arcadia-native.desktop ~/.local/share/applications/
install -m 0644 arcadia-native.png ~/.local/share/icons/hicolor/512x512/apps/
```

O `boot.mp4` fica ao lado do binário em `~/.local/bin`, que é onde o app o
procura pelo `current_exe()`. Sem ele, a interface abre direto.

Para verificar o download, rode `sha256sum -c SHA256SUMS` na pasta onde o
tarball e o checksum foram baixados.

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

O job `native` em `.github/workflows/ci.yml` roda em `ubuntu-22.04`, separado
dos jobs Tauri:

1. instala `pkg-config`, `libgstreamer1.0-dev`,
   `libgstreamer-plugins-base1.0-dev` e `libudev-dev`;
2. roda `cargo test -p arcadia-native`;
3. roda `./scripts/package-native-linux.sh` (build release + tarball);
4. publica o `.tar.gz` e o `SHA256SUMS` como artifact `arcadia-native-linux`.

O job `native-windows` roda em `windows-latest`, também independente dos jobs
Tauri:

1. baixa `gstreamer-1.0-msvc-x86_64-1.28.9.exe` do site oficial, confere o
   SHA-256 e instala com `/TYPE=devel /CURRENTUSER` em
   `%RUNNER_TEMP%\gstreamer`;
2. usa o `pkg-config` do próprio SDK GStreamer (instala `pkgconfiglite` pelo
   Chocolatey só se ele não vier);
3. exporta `GSTREAMER_ROOT_X86_64`, `PKG_CONFIG_PATH` e o `bin` no `PATH`;
4. roda `cargo check -p arcadia-native`;
5. roda `scripts/package-native-windows.ps1` e publica o `.zip` e o
   `SHA256SUMS` como artifact `arcadia-native-windows`.

Os jobs `rust`, `package`, `windows` (Tauri/NSIS) e `native` (Linux) não foram
alterados.

## Limitações conhecidas

- **Sem instalador e sem auto-update.** O artefato é um tarball/zip portátil;
  não há `.deb`, AppImage, NSIS nem updater para o nativo nesta etapa.
- **Windows sem assinatura.** O zip não é assinado; o SmartScreen pode avisar
  na primeira execução e não há atalho no menu Iniciar.
- **GStreamer embutido é opcional no Windows.** Se `GSTREAMER_ROOT_*` não
  estiver definido, o zip sai sem `gstreamer\` e exige GStreamer 1.28 MSVC
  x86_64 + VC++ 2015–2022 na máquina. Use `run-arcadia-native.cmd` quando o
  runtime for embarcado: o `.exe` sozinho não procura DLLs em `gstreamer\bin`.
- **Zip grande com GStreamer.** Copiar o runtime inteiro (`bin` + plugins)
  deixa o `.zip` na casa das centenas de MB; sem GStreamer ele fica pequeno.
- **No Linux, GStreamer vem do sistema.** O tarball não injeta codecs como o
  AppImage do Tauri; a distro precisa ter os plugins. Sem `hlsdemux`/decoder
  H.264, o player reporta o elemento ausente em vez de falhar em silêncio.
- **glibc do host de build.** O binário exige a glibc de quem compilou. O
  artefato oficial sai do CI em Ubuntu 22.04 (glibc 2.35); distros mais antigas
  podem recusar o executável.
- **Ícone reaproveitado do Tauri.** Se `src-tauri/icons/icon.png` mudar, o
  próximo empacotamento usa a versão nova; não há cópia versionada no crate
  nativo.
