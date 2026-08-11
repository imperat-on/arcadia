# Arquitetura do Arcadia

Visao geral do sistema: um launcher de jogos Electron + um backend Node
proprio que sincroniza dados por conta entre maquinas.

## Visao de alto nivel

```
+----------------+        HTTPS (Tailscale Funnel)        +-----------------+
| App Electron   | -------------------------------------> | Backend Node    |
| (desktop/PS5)  |                                        | (notebook)      |
+----------------+                                        | Express+SQLite  |
       |                                                  +-----------------+
       | arquivos locais (por conta)                            |
       v                                                       v
  ~/.local/share/arcadia/                                 data/arcadia.db
```

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

Servico unico Express + SQLite que e a fonte da verdade por conta. Detalhes
em [server/README.md](../server/README.md).

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

`library.json` (global, escrito pelo indexador `index.py`) lista todos os
jogos detectados na maquina. A **posse** (quais a conta ve) fica em
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
- Nada sensivel (API keys, etag, caches de sources) vai pra nuvem.
