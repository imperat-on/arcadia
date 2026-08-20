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

Continuam locais: trailers, downloads, biblioteca, configuracoes e todas as
chaves pagas (Hubcap, debrid e Steam). O servidor nunca recebe esses segredos.

Documentos de referencia:

- [Design da loja no servidor](superpowers/specs/2026-08-11-loja-servidor-design.md)
- [Plano de implementacao](superpowers/plans/2026-08-11-loja-servidor.md)

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
