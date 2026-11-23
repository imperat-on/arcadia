# Dependências para executar o Arcadia

O [guia de empacotamento](NATIVE-PACKAGING.md) lista as bibliotecas gráficas e
de vídeo para Linux e Windows. Os programas abaixo atendem aos recursos que
você usar.

| Recurso | Dependência |
|---|---|
| Integração Steam | Steam instalada |
| Seletores de pasta e arquivo no Linux | `zenity` ou `kdialog` |
| Torrent no Linux | Python com o módulo `libtorrent` |
| Torrent no Windows | `aria2c.exe`, baixado pelo Arcadia quando necessário |
| Trailers do YouTube | `yt-dlp` |
| Download de depots | DepotDownloader e .NET 9+, baixados pelo Arcadia quando necessário |

## Torrent no Linux

O pacote inclui `resources/torrent_rpc/main.py` e o código do worker. O
Arcadia procura um Python que consiga importar `libtorrent`, nesta ordem:
`ARCADIA_PYTHON`, o ambiente dedicado em
`~/.local/share/arcadia-torrent/bin/python` e os executáveis no `PATH`.

Confirme que o módulo está disponível com
`python3 -c 'import libtorrent; print(libtorrent.__version__)'`. Se a
distribuição oferece somente a biblioteca C++, instale o binding Python em
um ambiente virtual. Por exemplo, com `uv`:

```sh
uv venv --python 3.13 ~/.local/share/arcadia-torrent
uv pip install --python ~/.local/share/arcadia-torrent/bin/python libtorrent
```

## Trailers

Instale o [yt-dlp](https://github.com/yt-dlp/yt-dlp) no `PATH` ou aponte
`YTDLP_PATH` para o executável. O GStreamer do sistema também precisa dos
plugins de vídeo descritos no [guia de empacotamento](NATIVE-PACKAGING.md).
