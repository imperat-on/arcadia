# Empacotamento do frontend nativo (Linux)

Como buildar, rodar e empacotar o `arcadia-native` — o frontend em Rust + iced,
sem Tauri e sem webview. O pacote Tauri (`scripts/package-linux.sh`,
`.deb`/AppImage/RPM) é outro caminho e não é afetado por este documento; veja
[docs/PACKAGING-LINUX.md](PACKAGING-LINUX.md).

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

## Dependências de runtime

O tarball não embarca GStreamer. A distribuição do usuário precisa fornecer:

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

## Empacotar

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

## CI

O job `native` em `.github/workflows/ci.yml` roda em `ubuntu-22.04`, separado
dos jobs Tauri:

1. instala `pkg-config`, `libgstreamer1.0-dev`,
   `libgstreamer-plugins-base1.0-dev` e `libudev-dev`;
2. roda `cargo test -p arcadia-native`;
3. roda `./scripts/package-native-linux.sh` (build release + tarball);
4. publica o `.tar.gz` e o `SHA256SUMS` como artifact `arcadia-native-linux`.

Os jobs `rust`, `package` e `windows` (Tauri/NSIS) não foram alterados.

## Limitações conhecidas

- **Sem instalador e sem auto-update.** O artefato é um tarball portátil; não há
  `.deb`, AppImage nem updater para o nativo nesta etapa.
- **Windows fica para depois.** O frontend nativo é Linux-first; o pacote
  Windows existente continua sendo o do Tauri.
- **GStreamer vem do sistema.** O tarball não injeta codecs como o AppImage do
  Tauri; a distro precisa ter os plugins. Sem `hlsdemux`/decoder H.264, o player
  reporta o elemento ausente em vez de falhar em silêncio.
- **glibc do host de build.** O binário exige a glibc de quem compilou. O
  artefato oficial sai do CI em Ubuntu 22.04 (glibc 2.35); distros mais antigas
  podem recusar o executável.
- **Ícone reaproveitado do Tauri.** Se `src-tauri/icons/icon.png` mudar, o
  próximo empacotamento usa a versão nova; não há cópia versionada no crate
  nativo.
