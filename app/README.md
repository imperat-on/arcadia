# Arcadia App (Electron)

Launcher de jogos para Linux com duas UIs: **desktop** (janela) e
**console / Big Picture** (fullscreen, gamepad, TV-friendly).

## Estrutura

```
app/
  src/                # React renderer (desktop/ + ps5-launcher/)
  electron/
    main.js           # main process (biblioteca, IPC, indexador)
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
  test/               # node --test (48 testes, modulos puros)
  package.json
```

## Rodar (desenvolvimento)

```bash
cd app
npm install
npm run dev       # vite (renderer)
npm run electron  # electron main process
```

O backend precisa estar de pe. Aponte `electron/supabase/config.js` para
a URL do servidor (por padrao `https://zes.tail6e748d.ts.net`), ou use a
env `ARCADIA_SUPABASE_URL`.

## Testes

```bash
node --test    # modulos puros (sem Electron runtime)
```

## Conta e sync

- Login/cadastro pelo backend proprio (email + username + senha).
- Dados locais escopados por conta em `contas/<user>/`.
- No login roda `biblioteca.reconcile()` + `sources.reconcile()`.
- Fontes com API key (debrid/Hubcap) ficam locais, nunca sincronizam.
