# Arcadia App (Electron)

Launcher de jogos para Linux com duas UIs: **desktop** (janela) e
**console / Big Picture** (fullscreen, gamepad, TV-friendly).

## Estrutura

```
    contracts/            # contratos runtime + tipos compartilhados com a API
app/
  src/                # React renderer (desktop/ + ps5-launcher/)
  electron/
    main.js           # bootstrap, biblioteca, IPC e indexador
    library-store.js  # compatibilidade de leitura/escrita do formato local
    library-repository.js # repositório: leitura, posse por conta e escrita atômica
    index-service.js  # job deduplicado/timeout do index.py
    launch-resolver.js # política pura de comando/perfil de execução
    emulator-registry.js # catálogo, perfis, scanner seguro e argv de emuladores Linux
    emulator-status.js   # preflight local de BIOS/firmware sem executar binários
    emulator-runtime.js  # detecção read-only de sessões concorrentes em /proc
    launch-log.js      # logs rotacionados com fechamento seguro de descritor
    snapshot-service.js # snapshots locais versionados de saves
    diagnostics.js     # relatório local sem paths, credenciais ou rede
    plugins.js         # fachada compatível do registro/SDK de plugins locais
    plugins/            # manifest v1, registry atômico e capability SDK
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
    download-integrity.js # verificação e recuperação de downloads
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

O backend precisa estar de pé. Use `ARCADIA_API_URL` para apontar à API
gerenciada do Arcadia (as variáveis `ARCADIA_SUPABASE_URL`/`SUPABASE_URL`
continuam aceitas por compatibilidade). O URL é normalizado sem barras finais.

## Testes

```bash
npm test       # módulos puros (sem runtime do Electron)
# Os tipos e normalizadores compartilhados ficam em ../contracts.
npx tsc --noEmit
npm run build
```

Para rodar uma instalação isolada sem misturar estado com
`~/.local/share/arcadia`, defina `ARCADIA_DATA_DIR` antes do build/execução.

A especificação do manifest v1 e do registro local de plugins está em
[`docs/PLUGINS.md`](../docs/PLUGINS.md).

## Emuladores e ROMs

`emulator-registry.js` detecta PCSX2, RPCS3, Dolphin, PPSSPP, DuckStation,
RetroArch, melonDS e DeSmuME sem executar processos. Perfis, BIOS/core e pastas
ROM ficam em `ARCADIA_DATA_DIR/emulators.json`; resultados do scanner ficam
separados em `ARCADIA_DATA_DIR/roms.json`. A configuração global fica em
**Configurações → Emulação**; o assistente tenta detectar PATH, pastas Linux padrão,
AppImages e Flatpaks instalados localmente. Se necessário, **Explorar manualmente** abre o
seletor do executável; o guia também explica permissões de AppImage e wrapper/argv seguro
para Flatpak. Os jogos só mantêm sua ROM/argv local. O scanner aplica allowlist de extensões,
rejeita symlinks, agrupa sidecars/playlists e pode ser chamado sem
pasta para pesquisar as pastas persistidas. O modo Hydra é opt-in no main e
monta somente `cmd: string[]`; BIOS/firmware são apenas detectados localmente.

## Repositório da biblioteca

`electron/library-repository.js` é a camada única para a biblioteca local:
aceita o envelope versionado (e arrays legados), filtra `library.json` pelo
`owned_games.json` da conta ativa, materializa a posse ausente somente na
primeira leitura de uma conta e grava biblioteca/posse com `tmp + rename`.
Guest continua vendo o `library.json` inteiro e não cria `owned_games.json` na
raiz. O módulo não depende de Electron; um adaptador pequeno no main pode usá-lo
sem alterar os canais IPC existentes.

## Conta e sync

- Login/cadastro pelo backend proprio (email + username + senha).
- Dados locais escopados por conta em `contas/<user>/`.
- No login roda `biblioteca.reconcile()` + `sources.reconcile()`.
- Fontes com API key (debrid/Hubcap) ficam locais, nunca sincronizam.
