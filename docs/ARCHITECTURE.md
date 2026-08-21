# Arquitetura do Arcadia

Visao geral do sistema: um launcher de jogos Electron + um backend Node
proprio que sincroniza dados por conta entre maquinas.

## Visao de alto nivel

```
+----------------+        HTTPS (Tailscale Funnel)        +-----------------+
| App Electron   | -------------------------------------> | Backend Node    |
| (desktop/PS5)  |                                        | (notebook)      |
+----------------+                                        | Express+PostgreSQL |
       |                                                  +-----------------+
       | arquivos locais (por conta)                            |
       v                                                       v
  ~/.local/share/arcadia/                                 data/arcadia.db
```

## Fila de downloads

`downloadmanager.js` mantém a fila em arquivo atômico, retoma itens pausados e
agora agenda prioridade `[-10,10]` com FIFO determinístico. A política de
ordenação fica em `download-queue-policy.js`, separada dos processos Legendary
e DepotDownloader.

## Execução segura

`launch-resolver.js` resolve `gameId`, perfil e comandos legados sem spawnar
processos. O renderer não consegue substituir o comando de um jogo conhecido;
custom/Wine e integração SLSsteam continuam callbacks controlados pelo main.

## Emuladores Linux e lançamento por argv

`app/electron/emulator-registry.js` mantém um catálogo extensível de PCSX2,
RPCS3, Dolphin, PPSSPP, DuckStation, RetroArch, melonDS e DeSmuME; extensões de
catálogo fornecidas pelo código passam pela mesma normalização de id/candidatos.
O registry não executa processos: detecta candidatos no `PATH`, valida executáveis e
ROMs regulares (sem symlink), pesquisa pastas com allowlist de extensões, limita
recursão/resultados e agrupa sidecars (`cue/bin`, `mds/mdf`) e playlists M3U.
Perfis e pastas ROM são persistidos em `ARCADIA_DATA_DIR/emulators.json`, e o
índice local dos resultados em `ARCADIA_DATA_DIR/roms.json`; ambos usam escrita
temporária + rename atômico. A resolução devolve sempre `cmd: string[]`.

A resolução aceita, de forma aditiva, `launchMode: "hydra"`: PCSX2/DuckStation
usam `-batch -fullscreen --`, RPCS3 usa `--no-gui` e RetroArch usa `-f`, sem
interpretar shell. RetroArch exige um core libretro regular. `emulator-status.js`
detecta BIOS PS1/PS2 por tamanho/assinatura e firmware RPCS3 sem baixar dumps; o
lançamento principal bloqueia apenas PS1/PS2 sem BIOS configurado com
`BIOS_NOT_CONFIGURED`. `emulator-runtime.js`
inspeciona `/proc` sem criar processos e retorna `EMULATOR_ALREADY_RUNNING` para
impedir duas sessões concorrentes. Para RPCS3,
MDS usa o MDF sidecar e PKG só aponta para um EBOOT instalado; um PKG ainda não
instalado retorna `pkg_instalacao_necessaria` com código `PACKAGE_INSTALL_REQUIRED`,
nunca é passado cru ao emulador; MDS sem MDF retorna `DISC_SIDECAR_MISSING`.
Os handlers `emulators:*` expõem catálogo, perfis, status, busca local de ROMs e
resolução ao preload; `game:launch` chama tudo no main e ignora comandos
arbitrários do renderer.

A aba **Configurações → Emulação** mostra um card por sistema e abre o modal
individual de configuração de executável, BIOS/core, pastas ROM e argv-base.
`GameSettingsDialog` e o modo **Emulador (ROM/ISO)** de `AddGameDialog` ficam
somente com a associação jogo/ROM/argv-extra, sem duplicar o perfil global.
ROMs, BIOS, cores e configurações permanecem locais; somente
metadados do jogo custom são adicionados à biblioteca.

## Providers e pipeline da biblioteca

O `index.py` mantém a orquestração e a escrita atômica, enquanto `indexers/`
concentra transformações puras dos providers. O primeiro corte separa o parser
VDF e as construções Steam, Legendary, Heroic, Lutris e SLSsteam; esses módulos
não acessam Electron, rede ou estado global e podem ser exercitados com fixtures.

## Contratos compartilhados

A pasta `contracts/` contém a versão runtime (`index.js`) e as declarações
TypeScript (`index.d.ts`) dos payloads de biblioteca e sincronização. O main
process normaliza dados locais antes do IPC; o backend repete a normalização
antes de persistir `push_library`. Campos desconhecidos são preservados para
permitir evolução compatível, enquanto entradas inválidas são descartadas.

## Os dois mundos

### 1. App Electron (`app/`)

Launcher com duas UIs: desktop (janela) e console / Big Picture (fullscreen
com gamepad). Main process em `app/electron/main.js`, renderer React em
`app/src/`.

**Escopo por conta:** arquivos locais vivem em `contas/<user>/` (via
`app/electron/supabase/conta.js`). Trocar de conta troca o escopo.

**Cliente de rede:** `app/electron/supabase/client.js` e um shim fetch que
fala com o backend proprio (shape GoTrue/PostgREST). Os modulos
`auth.js`, `friends.js`, `sync.js`, `biblioteca.js`, `sources.js` usam esse
shim. O `session.json` (criptografado) persiste a sessao.

### 2. Backend (`server/`)

Servico unico Express + PostgreSQL que e a fonte da verdade por conta. Detalhes
em [server/README.md](../server/README.md).

## Catalogo no servidor

O backend centraliza o catalogo publico usado pela loja em rotas
`/catalog/v1/*`. Todas exigem o mesmo JWT das rotas de conta. O app consulta
essas rotas por `app/electron/catalog.js`; APIs pagas e credenciais do usuario
nao passam pelo proxy.

Os dados ficam na tabela generica `catalog_cache`:

| Coluna | Uso |
|---|---|
| `key` | Identidade validada do catalogo, como `popular` ou `items:2622380` |
| `data` | JSON pronto para o app |
| `at` | Epoch em segundos da ultima busca |

Uma entrada valida e servida diretamente. Uma entrada vencida e devolvida de
imediato e revalidada em background (stale-while-revalidate). Sem entrada, a
rota devolve `404`; o app usa o espelho ou o cache legado.

### Endpoints do catalogo

| Grupo | Rotas |
|---|---|
| Loja | `/popular`, `/genre`, `/items`, `/sushi`, `/manifests/:appid` |
| Hydra | `/sources/:id/games`, `/search` |
| Jogo | `/sysinfo/:appid`, `/meta/:appid`, `/hltb/:appid` |
| Conteudo | `/news`, `/fixes`, `/ryuu` |

Todos os caminhos acima usam o prefixo `/catalog/v1`.

### TTL e fallback

| Dado | TTL |
|---|---|
| Popular e sushi | 6h |
| Items, manifestos e fontes Hydra | 7d |
| Generos | 12h |
| Noticias | 30min |
| HLTB | 30d |
| Fixes e Ryuu | 6h |
| Sysinfo e meta | sem TTL |

Cada resposta recebida e gravada atomicamente em
`~/.local/share/arcadia/catalog_espelho/`. Consultas com query string usam
arquivos separados. A ordem de fallback e servidor, espelho do catalogo e
cache legado (`store_*_cache.json`, `sources/*.json` e equivalentes).

### Versao do payload

`sysinfo` e `meta` nao tem TTL, entao uma linha gravada antes de um campo novo
nunca revalidaria por tempo. Cada payload carrega um campo `v`
(`CATALOG_VERSAO` em `server/src/catalog-fetch.js`). Quando a versao em cache e
menor que a atual, a rota trata como cache vazio e rebusca na hora; se a fonte
externa estiver fora, o registro antigo ainda e servido em vez de `404`.

Ao adicionar um campo ao payload, suba a versao correspondente — e assim que as
linhas ja gravadas se corrigem sozinhas.

### Busca local e paginação offline

`app/electron/local-search.js` mantém `catalog_search_index.json` (envelope
`version=2`, aceitando envelopes legados `version=1`) no diretório de dados.
Cada página de `popular`, `catalog`, `genre` ou `search` já recebida é
normalizada e deduplicada por `appid`; o arquivo é escrito com `tmp + rename` e
nunca guarda `uris` de fontes Hydra. A próxima abertura hidrata o índice dos
espelhos e dos caches legados sem depender da rede. A busca remove
acentos/pontuação e usa ranking estável (exato, prefixo, termos e substring;
título e `appid` desempatem por chave).

Além do texto, cada entrada indexa facets reais: `launcher`, `genre`, `tag` e
o estado explícito `installed`. Valores são comparados sem acentos e sem
sensibilidade a maiúsculas; várias opções dentro de uma facet são OR, enquanto
facets diferentes são AND. O índice expõe `facets` com contagens, e o envelope
inclui `metadata` (`schema`, contagem, fontes, facets e `generated_at`). Estado
de instalação ausente permanece desconhecido e não casa com `installed=true`
nem `installed=false`; indexadores devem enviar um booleano quando souberem o
estado. Para o catálogo Steam, `launcher=steam` é inferido apenas no índice,
sem alterar o payload legado devolvido à UI.

`search` continua devolvendo somente o array histórico de jogos; filtros são
opcionais. `searchPage`/`page` adicionam `total`, `offset`, `limit`,
`has_more`, `next_offset`, `facets` e `index`. A ordenação de páginas é
`normalizedTitle` + chave estável (`source:id`), portanto a mesma coleção
produz as mesmas páginas independentemente da ordem de chegada. `stats` e
`metadata` permitem observar o índice sem ler seu arquivo diretamente.

`steamstore.search` e `store:suggest` consultam esse índice antes de iniciar
uma chamada externa, mantendo o payload legado `{ ok, jogos, ... }`; filtros de
busca são aditivos e não mudam esse contrato. Quando não há hit local, o
resultado remoto também é indexado. A rota paginada
`store:recent("all", limite, offset)` usa uma fatia ordenada do índice quando
a página não está no espelho, permitindo navegar no catálogo offline e
retornando os metadados/facets da fatia. A biblioteca continua sendo filtrada
no renderer sem mudar `library:get`; as mesmas regras puras estão disponíveis
em `searchLibrary`/`pageLibrary` para consumidores Electron futuros,
preservando o jogo completo no resultado.

Continuam locais: trailers, downloads, biblioteca, configuracoes e todas as
chaves pagas (Hubcap, debrid e Steam). O servidor nunca recebe esses segredos.

Documentos de referencia:

- [Design da loja no servidor](superpowers/specs/2026-08-11-loja-servidor-design.md)
- [Plano de implementacao](superpowers/plans/2026-08-11-loja-servidor.md)

## Comunidade: reviews e coleções

A API `/community/v1` vive no backend Express e usa a mesma autenticação JWT.
Reviews são filtradas por appid; coleções respeitam visibilidade, bloqueios e
propriedade dentro da consulta SQL. Reports e a fila de moderação compartilham a
migration `0003_community.sql`, sem alterar o envelope legado do catálogo.

No Electron, `app/electron/supabase/community.js` é o único cliente de rede:
o main mantém tokens, expõe canais IPC tipados e grava um cache atômico por conta.
Leituras públicas e privadas podem voltar marcadas como `offline` quando a rede
cai; uma resposta que atravessa troca de conta é descartada com
`error/code: "conta_trocada"` antes de tocar o cache seguinte. O renderer usa
`CommunityPanel.tsx` nas páginas de jogo para avaliações e listas, sem acesso a
`ipcRenderer`, credenciais ou caminhos locais.

## O que sincroniza por conta

| Dado | Como | Sensivel? |
|---|---|---|
| Avatar | upload pro bucket `avatars`, grava `avatar_url` | nao |
| Background | upload pro bucket `backgrounds` (imagem/video), grava `background_url` | nao |
| Conquistas | RPCs `sync_achievements`/`pull_achievements` (earliest-wins) | nao |
| Horas de jogo | `push_library`/`pull_library` (acumula deltas) | nao |
| Biblioteca (posse) | `owned_games.json` local + RPCs | nao |
| Amigos | tabela `friendships` + realtime WS | nao |
| Sources publicas | `user_sources` + RPCs (`{id, url, name}`) | nao |
| **API keys (debrid/Hubcap)** | **NUNCA sincronizam** | **SIM, fica local** |

## Repositório local da biblioteca

A leitura e a persistência local ficam em `app/electron/library-repository.js`.
O repositório separa o `library.json` global (escrito pelo indexador) do
`owned_games.json` escopado pela conta, aplica a regra de migração lazy e usa
`tmp + rename` tanto para a biblioteca versionada quanto para a posse. Isso
mantém a leitura tolerante a arquivos ausentes/corrompidos sem expor payloads
inválidos ao renderer. Guest é tratado como o estado histórico: vê todos os
jogos e nunca materializa posse na raiz; os handlers IPC continuam retornando o
mesmo array de jogos.

## Posse de jogos por conta

`library.json` (global, escrito pelo indexador `index.py`) usa um envelope
versionado (`version`, `generated_at`, `sources`, `games`) e lista todos os
jogos detectados na máquina. O leitor aceita arrays legados para upgrades sem
migração destrutiva. A **posse** (quais a conta vê) fica em
`owned_games.json` por conta. `readLibrary()` filtra o global pela posse
quando logado. Guest ve tudo (zero regressao). Detalhe: um jogo instalado
no disco que outra conta nao possui nao aparece na biblioteca dela.

## Sincronizacao

- **No login** (`SIGNED_IN`): `biblioteca.reconcile()` + `sources.reconcile()`.
- **Em mudancas**: `agendarPush()` (debounce 2s) apos adicionar/remover
  jogos, sources, horas ao fechar jogo.
- **Offline-first**: filas locais (`sync_queue.json`, watermarks em
  `sync_state.json`) guardam o delta; o push sobe quando conecta.

## Seguranca

- JWT em `.env` (nunca versionado), bcrypt para senhas.
- RLS substituido por filtro explicito por `user_id` no backend.
- Uploads validados por magic bytes + teto por bucket.
- API keys pagas, tokens de provedores e ETags locais nunca vao para o servidor.
