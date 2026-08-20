# Arcadia App (Electron)

Launcher de jogos para Linux com duas UIs: **desktop** (janela) e
**console / Big Picture** (fullscreen, gamepad, TV-friendly).

## Estrutura

```
contracts/            # contratos runtime + tipos compartilhados app/server
app/
  src/                # React renderer (desktop/ + ps5-launcher/)
  electron/
    main.js           # bootstrap, biblioteca, IPC e indexador
    library-store.js  # leitura versionada e compatível da biblioteca local
    index-service.js  # job deduplicado/timeout do index.py
    launch-resolver.js # política pura de comando/perfil de execução
    launch-log.js      # logs rotacionados com fechamento seguro de descritor
    snapshot-service.js # snapshots locais versionados de saves
    diagnostics.js     # relatório local sem paths, credenciais ou rede
    trailer-service.js # downloads/busca yt-dlp sem dependência de Electron
    preload.js        # ponte seguro renderer <-> main
    supabase/         # client do backend proprio (shim fetch)
      client.js       # shim: auth, REST-lite, rpc, storage, realtime WS
      auth.js         # cadastro/login/perfil/avatar/background
      friends.js      # amigos + realtime (badge)
      sync.js         # conquistas (offline-first)
      biblioteca.js   # sync de posse/horas
      sources.js      # sync de sources publicas (reconcile)
      conta.js        # escopo por conta (contas/<user>/)
      session.js      # sessao criptografada (session.json)
    owned.js          # posse de jogos por conta (owned_games.json)
    sources.js        # fontes de download (registro + caches)
    downloadmanager.js# fila de downloads
    steamstore.js     # loja Steam
  test/               # node --test (módulos puros e contratos)
  package.json
```

## Rodar (desenvolvimento)

```bash
cd app
npm install
npm run dev       # vite (renderer)
npm run electron  # electron main process
```

O backend precisa estar de pé. Use `ARCADIA_API_URL` para apontar ao servidor
Node (as variáveis `ARCADIA_SUPABASE_URL`/`SUPABASE_URL` continuam aceitas por
compatibilidade). O URL é normalizado sem barras finais.

## Testes

```bash
npm test       # módulos puros (sem runtime do Electron)
# O contrato de biblioteca é compartilhado com server/ via ../contracts.
npx tsc --noEmit
npm run build
```

Para rodar uma instalação isolada sem misturar estado com
`~/.local/share/arcadia`, defina `ARCADIA_DATA_DIR` antes do build/execução.

## Conta e sync

- Login/cadastro pelo backend proprio (email + username + senha).
- Dados locais escopados por conta em `contas/<user>/`.
- No login roda `biblioteca.reconcile()` + `sources.reconcile()`.
- Fontes com API key (debrid/Hubcap) ficam locais, nunca sincronizam.
