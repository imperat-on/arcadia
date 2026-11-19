# Empacotamento Linux (Tauri)

Como o Arcadia vira artefato distribuível no Linux, o que já está fechado e o
que ainda falta. Escrito contra o código, não contra a intenção.

## Comando

```bash
./scripts/package-linux.sh --verify
```

Faz, em ordem: build do front-end, `tauri build` para `.deb`, AppImage e RPM,
injeção dos codecs de vídeo no AppDir, reempacotamento do AppImage e
verificação. Sai com código != 0 se qualquer checagem falhar.

Artefatos em `target/release/bundle/{deb,appimage,rpm}/`.

## Por que existe injeção de codec

O `<video>` da UI roda no WebKitGTK, que no Linux **delega toda decodificação
de mídia ao GStreamer do sistema**. Isso difere do Chromium no Windows, que
embarca os decodificadores no próprio binário — é por isso que o app funcionava
lá e abria trailer em preto aqui.

A flag `bundle.appimage.bundleMediaFramework` do Tauri embarca os plugins
básicos (~194), mas deixa de fora justamente os patenteados:

| Plugin | Papel | Consequência sem ele |
|---|---|---|
| `libgstopenh264.so` | decoder H.264 | trailer não decodifica |
| `libgsthls.so` | demuxer HLS | trailer moderno da Steam (`.m3u8`) não abre |
| `libgstlibav.so` | decoder via ffmpeg | alternativa, arrasta ~130 libs |
| `libopenh264.so.8` | lib que `libgstopenh264` exige | o plugin não carrega |

O script injeta os três primeiros no AppDir e reempacota. Escolha por
`openh264` e não por `libav`: cobre H.264 com 143 KB em vez de embarcar o
ffmpeg inteiro.

## O que já está verificado

`scripts/verify-package.sh` prova, por execução (16 checagens):

- `.deb` declara `libwebkit2gtk-4.1-0`, `libgtk-3-0`, `gst-plugins-base/good/bad`
  e `gst-libav` — o `apt` resolve no usuário.
- `.rpm` declara `webkit2gtk4.1`, `gtk3`, `gstreamer1-plugins-base/good/bad-free`
  e `gstreamer1-libav` — nomes próprios do RPM, lista separada da do deb. Sem
  isso o Fedora instalava o app e todo trailer ficava preto.
- AppImage embarca `libgstopenh264.so`, `libgsthls.so`, `libopenh264.so.8`.
- Os plugins carregam **de dentro do pacote** (`gst-inspect` em ambiente limpo,
  com `GST_PLUGIN_PATH` apontando só para o AppDir).
- O pipeline `qtdemux ! h264parse ! openh264dec ! videoconvert` decodifica o
  `boot.mp4` usando só as libs do pacote.
- O binário não tem RPATH apontando para caminhos do host.

`crates/arcadia-updater/tests/packaging_contract.rs` trava as decisões que já
quebraram uma vez: versão do bundle igual à do workspace, `deb`/`appimage` nos
targets, dependências declaradas, `beforeBuildCommand` resolvendo da raiz, e a
presença da injeção de codec no script.

## Limites conhecidos

### glibc — por que este AppImage não roda em "todas as distros"

O AppImage compilado nesta máquina (Arch, glibc 2.44) exige **GLIBC_2.39** no
binário. E o motivo é mais duro: as libs embarcadas arrastam a glibc do host
onde foram construídas.

Medido no AppImage gerado aqui:

    libwebkit2gtk-4.1.so.0   GLIBC_2.44
    libglib-2.0.so.0         GLIBC_2.43
    libgtk-3.so.0            GLIBC_2.38

Ou seja: **Ubuntu 24.04+, Fedora 39+, Arch**. Ubuntu 22.04 (LTS) e Debian 12
não abrem — o loader falha antes de qualquer código do app rodar.

**Isso não se resolve baixando a glibc alvo do binário.** Testado: com
`cargo-zigbuild --target x86_64-unknown-linux-gnu.2.28` o link falha em

    ld.lld: error: undefined reference: dlopen@GLIBC_2.34
    >>> referenced by /usr/lib/libgmodule-2.0.so

O binário compilaria, mas as libs do sistema que ele linka já exigem glibc 2.44.
Só reduzir a exigência do binário não muda as dependências dinâmicas.

O único caminho correto é **empacotar num host com glibc antiga**:

| Método | Resultado |
|---|---|
| CI em `ubuntu-22.04` | GLIBC_2.35 → Ubuntu 22.04+, Debian 12+, Fedora 36+, Arch |
| Container/VM com distro antiga | mesmo efeito, controlado por você |

O job `package` do CI já roda em `ubuntu-22.04` de propósito: o AppImage
publicado lá é o distribuível. **O AppImage gerado nesta máquina (Arch) serve
para testar, não para publicar.**

Flatpak, se um dia voltar à mesa, elimina isso de uma vez, junto com o codec.

### NVIDIA proprietário

Com driver proprietário, o compositing por dmabuf do WebKitGTK renderiza o
vídeo como retângulo verde chapado (áudio e timeline normais). O
`arcadia-tauri.sh` exporta `WEBKIT_DISABLE_DMABUF_RENDERER=1` por padrão, o que
força o caminho de cópia por CPU. `ARCADIA_KEEP_DMABUF=1` desliga o workaround.

### `.deb` e `.rpm` não embarcam codec

Os dois **declaram** a dependência e deixam `apt`/`dnf` resolver. É o
comportamento correto: embarcar libs de sistema num pacote nativo briga com o
gerenciador de pacotes da distro. Quem precisa de zero dependência usa o
AppImage, que embarca os codecs.

## Pendências

- **Auto-update não existe no port Rust.** `arcadia-updater` tem o canal git e
  o canal empacotado portados (2.971 linhas, com `AutoUpdater` como trait
  injetada), mas **nenhum crate o importa**. Os canais IPC de update em
  `arcadia-ipc` devolvem `nao_suportado` de propósito. Ligar isso exige
  implementar o `AutoUpdater` concreto (HTTP + verificação de assinatura +
  substituição do AppImage) e ligar o estado ao IPC. É trabalho de projeto, não
  um ajuste — até lá o app não se atualiza sozinho e não finge que se atualiza.
- **Assinatura de código.** Nem Windows nem Linux têm assinatura. No Windows o
  SmartScreen avisa; no Linux, um AppImage sem assinatura de release não é
  verificável pelo usuário.
