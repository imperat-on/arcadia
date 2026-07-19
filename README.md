# Arcadia

Front-end de jogos para Linux (Electron), com dois modos de interface:

- **Desktop** (estilo Heroic): biblioteca, loja, downloads, gerenciador de Wine
- **Console** (estilo PS5, tela cheia + gamepad)

## Recursos

- **Biblioteca unificada** — Steam, Epic (Legendary), jogos custom (.exe via Wine) com capas, favoritos, categorias e página de detalhes por jogo
- **Loja Steam** — busca no catálogo (Hubcap/Morrenus + fallback), download via **DepotDownloader** direto para as bibliotecas Steam (multi-drive, lê `libraryfolders.vdf`), ou "Add" para baixar pela própria Steam via **SLSsteam**; fallback de manifestos em 4 provedores (Morrenus, Ryuu, TwentyTwo Cloud, Sushi); fixes de jogos (GameBypass/OnlineFix)
- **Lançamentos configuráveis por jogo** — versão do Wine/Proton (incl. Proton via UMU), prefixo por jogo, DXVK/NVAPI/VKD3D, Esync/Fsync, gamescope, gamemode, MangoHud, wrappers, variáveis de ambiente, argumentos, scripts pré/pós-jogo e logs
- **Downloads** — fila serial (Epic via Legendary, Steam via DepotDownloader) com progresso real em MiB, velocidade e ETA; pausar/retomar/cancelar (apaga parciais)
- **Wine Manager** — instala/gerencia GE-Proton e Wine-GE; Protons da Steam detectados
- **Conquistas** — toasts estilo PS5 e integração SLScheevo
- **Trailers** — busca/download de trailers via YouTube (yt-dlp)

## Requisitos

- Linux x86_64, **python3**, **Steam** (nativa), **.NET 9+** (o app instala localmente se faltar)
- Para jogos Epic: login no **Legendary** (o app baixa o binário)
- Para a Loja Steam: **API key do Hubcap** (grátis na comunidade) e **SLSsteam** (instalável por botão em Configurações → Integrações)

## Instalação

```bash
git clone <repo> && cd arcadia
cd app && npm install
cd ..
cp config.example.json config.json   # preencha suas chaves
./install-desktop.sh                  # opcional: instala no menu de aplicativos
./arcadia-desktop.sh                  # modo desktop
./arcadia.sh                          # modo console (tela cheia)
```

Na primeira execução, o `arcadia.sh` indexa a biblioteca (`index.py`) e compila o front-end (`npm run build`).

## Configuração

`config.json` (não versionado — ver `config.example.json`):

| Chave | Para quê |
|---|---|
| `steam_api_key` | Biblioteca completa da Steam (jogos possuídos) |
| `hubcap_api_key` | Busca/download de manifestos na aba Lojas |

## Estrutura

```
app/src        # front-end React (desktop/ + ps5-launcher/)
app/electron   # processo principal (main.js, downloadmanager, steamstore, winemanager)
index.py       # indexador da biblioteca (Steam/Heroic/Lutris → library.json)
arcadia.sh     # entrada (console) · arcadia-desktop.sh (desktop)
```

Os dados do usuário (config, biblioteca, downloads, prefixos, artes) ficam em
`~/.local/share/arcadia/` e **não** são versionados.

## Aviso legal

Projeto pessoal de interoperabilidade. Respeite os termos de serviço da Valve,
Epic Games e demais plataformas. Use por sua conta e risco.
