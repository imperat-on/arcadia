# Arcadia → Rust — Plano de port

**Repositório:** `arcadia-rust` (fork de `imperat-on/arcadia`, branch `rust-port`)
**Base:** v1.5.5 — commit `81fd3ae`
**Status:** plano aprovado para execução por fases
**Regra de ouro:** o servidor não é tocado. Nenhum schema, migration, DELETE ou contrato novo. O cliente Rust fala exatamente o protocolo que o cliente Node fala hoje.

---

## 1. Veredito

É possível, sim — com uma condição de arquitetura: o renderer (React + CSS) continua sendo o renderer. O que vira Rust é o processo principal inteiro: janela, IPC, rede, disco, launch, downloads, sync, achievements, retrô, emuladores, plugins, updater.

Isso entrega o que foi pedido:

- **Backend 100% Rust** — 38,6 mil linhas de Electron/Node substituídas por crates.
- **Visual literalmente igual** — o DOM é o mesmo, o CSS é o mesmo, os componentes são os mesmos. Nenhuma linha de `app/src` reescrita. O Chromium do Electron vira WebView2 (Windows, mesmo motor Blink) e WebKitGTK (Linux).
- **Dados do usuário intactos** — servidor intocado, `dataDir` lido no mesmo formato, backup automático antes da primeira escrita, escrita atômica, nenhum campo desconhecido descartado.

Se "portado pra Rust" exigir também **zero JavaScript no renderer**, o caminho é o Plano B (seção 3.2): Dioxus/WASM com o mesmo CSS. É possível, mas é outro projeto em cima deste, não um pré-requisito.

---

## 2. Números do sistema atual

| Item | Valor |
|---|---|
| Linhas JS/TS/CSS (sem `node_modules`) | ~70.500 |
| Processo principal (`app/electron`) | 38,6 mil linhas, 100 arquivos |
| Renderer (`app/src`) | 30,0 mil linhas TSX/TS + 2,5 mil linhas CSS |
| Canais IPC | **202** `ipcRenderer.invoke` ↔ **202** `ipcMain.handle` |
| Canais de evento (main → renderer) | **22** |
| Métodos da API usados pela UI | 148 de ~200 expostos |
| Testes existentes | 119 arquivos (`node --test`) — servem de oráculo |
| Servidor | API REST própria: `auth/v1`, `rest/v1`, `storage/v1` + 12 RPCs |
| Tabelas usadas | `profiles`, `friendships`, `avatars` |
| Binários externos | aria2c, yt-dlp, DepotDownloader (.NET), legendary, Python (torrent RPC), wine/proton, umu-run, gamescope, gamemoderun, 7z, tar |
| Dados locais (`%LOCALAPPDATA%\arcadia`) | `config.json`, `session.json`, `library.json`, `contas/<user>/`, `achievement_icons/`, `hltb_cache.json`, `plugins/`, `appcache/`, `steamapps/`, `downloads/` + prefixos/jogos/trailers |

---

## 3. Arquitetura alvo

```
┌──────────────────────────────────────────────────────────────┐
│ ui/  (React 18 + Tailwind 4 + Vite — copiado de app/src)      │
│      inalterado. Fala window.launcherAPI.*                    │
│      inalterado. Recebe eventos via onXxx(cb).                │
└──────────────────────────┬───────────────────────────────────┘
                           │ shim gerado (initialization script)
                           │ launcherAPI → invoke("cmd") / listen("evento")
┌──────────────────────────▼───────────────────────────────────┐
│ src-tauri (binário arcadia)                                   │
│  janela frameless · fullscreen console · single instance      │
│  tray · toasts · zoom/escala · boot video · updater            │
├──────────────────────────────────────────────────────────────┤
│ crates/                                                       │
│  arcadia-ipc        registro dos 202 comandos + 22 eventos    │
│  arcadia-contracts  port de contracts/index.js + sync.js      │
│  arcadia-api        auth/rest/rpc/storage + DoH + IPv4/fallback│
│  arcadia-store      config/session/library/overrides/settings │
│  arcadia-launch     launch-resolver, wine/proton, gamescope,  │
│                     focus-session, process-tools              │
│  arcadia-download   dm, aria2 RPC, torrent bridge, integridade│
│  arcadia-steam      steam-path/vdf/account/horas/news/store   │
│  arcadia-achievements  watchers, cracked, uplay, consolidar   │
│  arcadia-retro      catálogo, matcher, offers, repository     │
│  arcadia-emulators  registry, runtime, status, ROM index      │
│  arcadia-media      artes, trailers, HLTB, IGDB/PSN/Xbox/SGDB │
│  arcadia-plugins    manifest/registry/trust/sdk (sem execução)│
│  arcadia-updater    canal git + canal empacotado              │
│  arcadia-diagnostics  logs, suporte, sysinfo, diagnóstico     │
└──────────────────────────────────────────────────────────────┘
         │ spawn idêntico (args/env/stdout)
         ▼
   aria2c · yt-dlp · dotnet DepotDownloader · legendary
   python torrent_rpc · wine/proton · umu-run · gamescope
```

### 3.1 Stack

| Camada | Escolha | Motivo |
|---|---|---|
| Shell nativo | **Tauri 2** (wry/tao) | Janela nativa + webview, plugins de diálogo/opener/single-instance/updater, binário pequeno, IPC tipado |
| Renderer | React 18 + Tailwind 4 + Vite (intacto) | Paridade visual garantida |
| WebView | WebView2 (Win) / WebKitGTK (Linux) | Blink no Windows = mesmo motor do Electron |
| Async | tokio | Rede/processos concorrentes |
| HTTP | reqwest + rustls + hickory-resolver (DoH) | Port do `httpfetch.js` (primário + fallback, IPv4 forçado, DoH Cloudflare) |
| JSON | serde/serde_json com `#[serde(flatten)]` | Preserva campos desconhecidos — regra de não perder dados |
| Erros | thiserror (bibliotecas), anyhow (borda) | Sem string solta em contrato |
| Logs | tracing + tracing-appender | Port do `debug.js`/`launch-log.js` |
| Segredos | `windows` (DPAPI) / Secret Service (Linux) | Ler o `session.json` atual do Electron sem exigir novo login |
| Processos | std::process + tokio + Job Objects (Win) | Matar árvore de processos como hoje |
| Gamepad | Web Gamepad API (Win); gilrs fallback (Linux) | O renderer já tem `useGamepadNav` |
| Empacotamento | Tauri NSIS + AppImage | Mesmos formatos de hoje |

### 3.2 Plano B — renderer 100% Rust (opcional, decisão separada)

Portar `ui/` para **Dioxus** (Rust → WASM) mantendo o mesmo `index.css`, classes Tailwind e estrutura de componentes. O visual pode chegar a idêntico, mas cada tela precisa ser re-provada e os 148 métodos da API remapeados. Esforço adicional estimado: **+80 a 120 dev-days**. Não bloqueia o Plano A; o binário Rust do Plano A já é o produto final funcional.

---

## 4. Mapa de port (arquivo → crate)

O diretório `app/` do fork fica **congelado** como oráculo. Nada é editado nele. O código novo vive em `ui/`, `src-tauri/`, `crates/` e `scripts/`.

| Origem (`app/electron/`) | Destino | Observações |
|---|---|---|
| `main.js` (6.111) | `arcadia-app` + `arcadia-ipc` | quebra por domínio; não vira um arquivo só |
| `preload.js` | `shim/launcher-api.js` (gerado) | mapeamento 1:1 dos 202 canais |
| `runtime-paths.js` | `arcadia-store::paths` | mesmo `ARCADIA_DATA_DIR`, mesmos caminhos |
| `secret-config.js`, `overrides.js`, `library-store.js`, `library-repository.js`, `owned.js` | `arcadia-store` | JSON com campos preservados |
| `supabase/config.js`, `client.js`, `auth.js`, `session.js`, `conta.js` | `arcadia-api` + `arcadia-store` | REST shim portado; DPAPI/Secret Service |
| `supabase/biblioteca.js`, `sync.js`, `friends.js`, `community.js`, `ipc.js`, `sources.js` | `arcadia-api` + `arcadia-ipc` | mesmos RPCs e envelopes |
| `contracts/index.js`, `sync.js` | `arcadia-contracts` | testes de fixture idênticos |
| `launch-resolver.js`, `launch-log.js`, `game-session.js`, `focus-session.js`, `window-focus.js`, `process-tools.js`, `gamepad-launch.js`, `gamescope-session.js`, `umu-runtime.js`, `wine*` | `arcadia-launch` | Windows e Linux; Win32 API no lugar de PowerShell |
| `downloadmanager.js`, `download-integrity.js`, `download-queue-policy.js`, `aria2.js`, `depotdownloader.js`, `torrent.js` | `arcadia-download` | protocolo aria2 JSON-RPC igual; Python inalterado |
| `steam-path.js`, `vdf.js`, `steam-account.js`, `steam-horas-servidor.js`, `steam-news.js`, `steamstore.js`, `catalog.js`, `hosters.js` | `arcadia-steam` | registry do Windows via crate `windows` |
| `achievements/*` (8 arquivos), `retroachievements/*` | `arcadia-achievements` | watchers com `notify`; parsers idênticos |
| `retro-*.js` (14 arquivos) | `arcadia-retro` | catálogo/matcher/repositório |
| `emulator-*.js` (3) | `arcadia-emulators` | |
| `metadata.js`, `trailer-service.js`, `hltb.js`, `news.js` | `arcadia-media` | providers SGDB/PSN/Xbox/IGDB |
| `plugins/*` (5) | `arcadia-plugins` | registry/trust/manifest; nenhum entry é executado |
| `updater.js`, `updater-packaged.js` | `arcadia-updater` | git channel + tauri-plugin-updater |
| `diagnostics.js`, `diagnostic.js`, `support-bundle.js` | `arcadia-diagnostics` | mesmo formato de bundle |
| `notify.js`, `bootVideo.js`, `ui-scale.js`, `startup.js`, `fixes.js`, `local-search.js`, `game-settings-service.js`, `snapshot-service.js`, `sources.js` | crates conforme domínio | |
| `scripts/gamepad-xinput.py`, `torrent_rpc/*.py` | `scripts/` (inalterado) | copiados sem alteração |

---

## 5. Contratos intocáveis

### 5.1 IPC

- 202 canais nomeados exatamente como hoje (`library:get`, `game:launch`, `dm:progress`, …).
- O shim é **gerado** a partir de `preload.js` (parser + tabela `canal → comando`), não escrito à mão. Um teste falha se qualquer canal do preload não tiver handler Rust e vice-versa.
- `window.launcherMode`, `window.launcherPlatform`, `window.launcherPaths` expostos pelo shim com os mesmos valores.
- Nenhuma mudança em `ui/src` além de (se necessário) uma linha de CSP no `index.html`.

### 5.2 Eventos (22)

`ui:escala`, `torrent:progress`, `dm:progress`, `trailer:progress`, `trailer:dlprogress`, `app:focus`, `game:running`, `game:active`, `game:launchState`, `achievement:unlocked`, `library:changed`, `plugins:changed`, `update:available`, `update:progress`, `update:packaged:changed`, `store:downloaded`, `game:launchError`, `game:launchWarning`, `account:changed`, `friends:request`, `friends:changed`, `sync:state`.

Mesmos nomes, mesmos payloads, mesma semântica de unsubscribe.

### 5.3 Servidor (não tocar)

- `POST /auth/v1/token?grant_type=password|refresh_token`, `/auth/v1/signup`, `/auth/v1/logout`, `GET /auth/v1/user`, `GET /auth/v1/health`.
- `/rest/v1/profiles|friendships|avatars` com os mesmos query params PostgREST (`select`, `eq`, `order`, `limit`, upsert via `Prefer`).
- `/rest/v1/rpc/`: `login_check`, `username_available`, `status`, `push_library`, `pull_library`, `sync_achievements`, `pull_achievements`, `push_steam_playtime`, `pull_steam_playtime`, `push_sources`, `pull_sources`, `friend_achievements`, `friend_profile`.
- `/storage/v1/object/avatars/...` com os mesmos headers.
- Headers: `apikey` fixo + `authorization: Bearer <access_token>`; refresh automático no 401, exatamente como hoje.
- **Zero** migrations, **zero** novos endpoints, **zero** DELETEs não iniciados pelo usuário.

### 5.4 Dados locais

- Mesmo `dataDir`, mesmos nomes de arquivo, mesmo JSON.
- `session.json` formato `{v:2, enc:true, data:<base64>}` lido do disco atual: DPAPI no Windows, Secret Service/AES no Linux. Se a decriptação falhar, o usuário reloga — nenhum dado de servidor é perdido.
- Escrita sempre atômica (tmp + rename + fsync), permissão 0600 onde aplicável.
- Campos desconhecidos preservados (`serde(flatten)` + round-trip test).
- Antes da primeira escrita: cópia automática `backup-pre-rust-<timestamp>/` de `config.json`, `session.json`, `library.json`, `contas/`.
- Lockfile em `dataDir` para impedir Electron antigo e Rust novo escrevendo ao mesmo tempo.

### 5.5 Binários externos

Protocolos de linha de comando e JSON-RPC preservados byte a byte. Nada é reimplementado em Rust: `aria2c` continua sendo aria2c, `yt-dlp` continua yt-dlp, `DepotDownloader` continua .NET, `torrent_rpc` continua Python. O Rust só orquestra.

---

## 6. Garantia de dados do usuário

### 6.1 Servidor (crítico)

1. **Servidor fora do escopo.** Nenhum acesso de escrita ao repositório do backend, nenhuma migration, nenhum deploy.
2. **Paridade comprovada antes de qualquer push.** O cliente Rust roda primeiro em modo `pull-only` (`ARCADIA_SYNC_READONLY=1`). Só habilita push quando os goldens de request/response baterem.
3. **Goldens reais.** Fase 0 grava os requests do cliente Node (proxy local de captura) para login, pull/push de biblioteca, achievements, playtime, sources, friends e community. Os testes Rust comparam método, path, query, headers e corpo canônico.
4. **Mesma resolução de conflito.** `contracts/sync.js` portado linha a linha, com os fixtures dos testes JS atuais como casos de aceitação.
5. **Nenhuma operação destrutiva nova.** Remoções continuam sendo só as ações explícitas do usuário (remover jogo, desfazer amigo, apagar review), com os mesmos payloads.
6. **Sessão preservada.** O Rust lê o `session.json` existente; refresh token continua o mesmo fluxo. Contas, bibliotecas, conquistas e playtime no servidor permanecem exatamente como estão.

### 6.2 Local

1. O Rust abre o mesmo `dataDir` — não cria um diretório novo, não importa/exporta, não reescreve em massa.
2. Round-trip: ler `library.json`/`config.json`/`contas/` e regravar semântica e estruturalmente idêntico (teste de hash normalizado).
3. Backup automático antes da primeira escrita (uma vez por versão).
4. Escrita atômica em todos os arquivos de estado; nunca truncar arquivo em uso.
5. Lock de instância única + lockfile de data dir.

### 6.3 Critério de aceitação de dados

- Rodar o Rust sobre um perfil Electron real: **diff de árvore** antes/depois (excluindo logs e caches voláteis) = zero mudanças inesperadas.
- Login com conta existente: `GET /auth/v1/user` idêntico; pull de biblioteca idêntico ao do Node.
- Push de uma alteração trivial (ex.: playtime de um jogo) e verificar no servidor que só aquele registro mudou.
- Nenhum 4xx/5xx novo no backend.

---

## 7. Fases

Esforço em dev-days (uma pessoa focada; agente acelera digitação, não a validação em máquina real).

### Fase 0 — Fundação e oráculo (3–5 dias)

- Congelar `app/`; criar workspace Cargo, `ui/` (cópia de `app/src` + `public` + `index.html`), `scripts/`.
- Extrair a tabela `canal → handler` do preload; gerar o shim; teste de cobertura 202/202.
- Portar `contracts/` para `arcadia-contracts` com os fixtures dos testes JS.
- Harness de captura HTTP (proxy) e de screenshot (Electron vs Tauri).
- **DoD:** workspace compila vazio, teste de contrato do IPC passa, fixtures versionados.

### Fase 1 — Shell Tauri e persistência (8–12 dias)

- Janela frameless 1600×900, `ready-to-show`, fullscreen console, modo desktop/console, F11.
- Single instance, tray (`minimize_to_tray`/`close_to_tray`), escala/zoom por monitor, boot video.
- `arcadia-store`: paths, config, session (DPAPI), library, overrides, game settings, snapshot.
- Toasts de conquista (janela transparente, always-on-top, click-through).
- Diálogos (pasta/arquivo), opener, notificações.
- **DoD:** UI React abre no Tauri, todas as telas navegam, config/session/library leem e regravam idêntico, backup automático funcionando.

### Fase 2 — Servidor, conta e biblioteca (15–20 dias)

- `arcadia-api`: auth completo, refresh, REST, RPC, storage, DoH + IPv4 + fallback de endpoint.
- `biblioteca.js`, `friends.js`, `community.js`, `sync.js`, `sources.js` portados.
- Envelopes seguros (`safeAccountSession`, `safeAuthResult`, …) idênticos.
- Modo pull-only e harness de paridade.
- **DoD:** login/cadastro/perfil/amigos/community funcionam; pull/push batem com os goldens; nenhum dado alterado no servidor fora do esperado.

### Fase 3 — Launch pipeline (18–25 dias)

- `launch-resolver` completo; Windows nativo; Linux com Proton/Wine, prefixos, DXVK/NVAPI/VKD3D, esync/fsync, gamescope, gamemode, MangoHud, wrappers, env, args, pre/post scripts.
- Process tracking, kill tree, `game:running`/`game:active`/`launchState`, focus session, window focus (Win32 `SetWinEventHook`; Linux X11/Wayland conforme suporte atual).
- Saves/snapshots, uninstall, import, sysinfo, ProtonDB.
- **DoD:** jogo real abre e fecha pelos dois caminhos (Steam e exe), estado da UI reflete corretamente, zero processos órfãos.

### Fase 4 — Downloads (12–18 dias)

- DM queue/policy, staging, integridade, pause/resume/cancel, limpeza.
- aria2 JSON-RPC; DepotDownloader; torrent bridge Python (inalterada); debrid gate.
- Progresso `dm:progress`/`torrent:progress` idêntico (MiB, velocidade, ETA).
- **DoD:** fila com 2 downloads simultâneos, pause/resume, restart do app retoma o estado como hoje.

### Fase 5 — Steam (8–12 dias)

- Detecção de instalação, VDF, contas, horas, captura, notícias, Steam news images.
- Loja: iframe `store.steampowered.com` com CSP igual; sem webview legado.
- `steamstore` (busca/detalhes/install info/chave hubcap) e catálogo.
- **DoD:** horas por conta corretas, loja abre e navega, install info igual.

### Fase 6 — Achievements (15–20 dias)

- Watchers Steam/cracked, loader, match, schema, consolidar, uplay, Steam bins.
- RetroAchievements (login, status, progresso, apply to emulator).
- Toasts e `achievement:unlocked`; sync de conquistas.
- **DoD:** conquista desbloqueada em jogo real aparece no toast e sincroniza; conflitos resolvidos como no Node.

### Fase 7 — Retrô e emuladores (15–20 dias)

- Catálogo retrô (v1/v2), matcher, parser de título, sistemas, offers, repository, enriquecimento/metadata.
- Emulator registry/runtime/status, ROM index, perfis, config RA.
- **DoD:** biblioteca retrô importa, ROM roda com emulador detectado, metadados iguais.

### Fase 8 — Mídia e metadados (8–12 dias)

- Artes (SGDB, PSN, Xbox, IGDB, Steam, SteamGridDB), trailer service (yt-dlp), HLTB, news.
- Cache e `local-search`.
- **DoD:** capas/hero/logo resolvem e cacheiam; trailer baixa e toca; HLTB responde.

### Fase 9 — Plugins, fixes, diagnósticos, updater (10–14 dias)

- Registry/trust/manifest/sdk; built-ins (slssteam, luatools) como flags; nenhum entry executado.
- Fixes, diagnostics/support bundle, updater git e empacotado (assinatura/manifesto).
- **DoD:** plugin registra/valida/ativa igual; update empacotado baixa e instala; bundle de suporte abre.

### Fase 10 — Paridade, migração e soak (10–15 dias)

- Screenshot diff de todas as telas (desktop + console) Electron vs Tauri.
- Migração sobre perfil real + backup + rollback documentado.
- Teste de 72h: boot, jogo, download, conquista, sync, update.
- Empacotamento NSIS/AppImage, ícones, assinatura.
- **DoD:** checklist completo, zero regressão visual de layout, dados intactos, build reproduzível.

**Total: ~120–175 dev-days.** Em calendário: 4–6 meses solo em tempo integral; menos com sessões longas de execução assistida, mas a validação em máquina real (Steam, jogos, rede) é o gargalo, não a escrita.

---

## 8. Testes e paridade

- **Contrato IPC:** teste que compara o conjunto de canais do `preload.js` congelado com os comandos registrados no Rust. Falha = canal faltando ou sobrando.
- **Contratos puros:** fixtures dos 119 testes JS reaproveitados; portes puros (sync, queue policy, integridade, matchers, parsers, i18n) rodam os mesmos casos.
- **HTTP goldens:** gravação no cliente Node, replay no Rust, diff de corpo canônico e semântica de resposta.
- **Local round-trip:** hash normalizado de `library.json`, `config.json`, `contas/` antes/depois.
- **E2E visual:** `tauri-driver` + screenshots; comparação por região com tolerância só de antialiasing. Layout, posição, cor e tipografia têm que bater.
- **E2E funcional:** Windows 11 (Steam nativa) e Linux (VM com Steam/Proton), matriz de jogos nativos + Proton + emulador.
- **Soak:** 72h com uso real; foco em vazamento de processos, locks e escrita concorrente.

---

## 9. Riscos e mitigações

| # | Risco | Impacto | Mitigação |
|---|---|---|---|
| R1 | WebKitGTK no Linux ≠ Blink (fontes, `backdrop-filter`, codecs) | Paridade visual no Linux | Gate na Fase 10; se necessário, empacotar WebKitGTK recente ou trocar o shell para CEF (`cef-rs`) mantendo o mesmo renderer |
| R2 | H.264 do `boot.mp4` ausente em alguns WebKitGTK | Boot sem vídeo no Linux | Fallback WebM VP9 + poster; teste por distro |
| R3 | `file://` não carrega em página servida por `tauri://localhost` | Mídia local quebrada | Já quase tudo passa por bytes/blob (`avatarLoad`, `trailerStreamUrl`); para os 14 usos restantes, asset protocol com escopo + rewrite no shim |
| R4 | `safeStorage` (DPAPI/Secret Service) | Sessão exige novo login | Implementar DPAPI no Windows (nativo) e OSCrypt/Secret Service no Linux; fallback = relogar, sem perda de dados |
| R5 | CSP do `index.html` bloqueia IPC do Tauri | App não fala com o backend | Mover CSP para `tauri.conf` e liberar `ipc:`/`http://ipc.localhost`; testar cedo na Fase 1 |
| R6 | Gamepad API no WebKitGTK | Navegação console quebrada no Linux | Fallback `gilrs` → eventos Tauri → adapter no `useGamepadNav` |
| R7 | Zoom por monitor não existe 1:1 no Tauri | Escala errada | Usar `set_zoom` do wry; fallback CSS `zoom` no root via evento `ui:escala` |
| R8 | Updater empacotado muda de formato | Auto-update quebrado | `tauri-plugin-updater` + gerar `latest.json` no release; canal git portado com `git2`/spawn |
| R9 | Duas versões (Electron e Rust) escrevendo no mesmo `dataDir` | Corrupção | Lockfile no dataDir + detecção de processo antigo + single-instance |
| R10 | Porte de 6 mil linhas de `main.js` com efeitos colaterais | Regressões silenciosas | Quebra por domínio com testes por crate; `app/` congelado como referência; feature flags por subsistema |

---

## 10. Decisões abertas

1. **Plano B (UI em Rust/Dioxus)** — entra agora ou fica como fase futura? Recomendação: não bloquear. O Plano A já entrega o app inteiro em Rust no runtime.
2. **Linux na v1 do port** — paridade visual no WebKitGTK exige validação em VM real; se o alvo imediato for Windows, o Linux entra na Fase 10.
3. **Assinatura de código do instalador Windows** — necessária para release público sem SmartScreen; hoje o Electron usa NSIS sem assinatura.
4. **Repositório remoto do fork** — hoje `upstream` aponta para o clone local. Definir o remote GitHub quando o primeiro build empacotado existir.

---

## 11. Próximos passos imediatos

1. Fase 0: workspace Cargo + `ui/` + shim gerado + `arcadia-contracts` com fixtures.
2. Provar o caminho crítico em uma spike de 1 dia: janela Tauri carregando o React atual, `library:get` respondendo pelo Rust com o `library.json` real, e imagem local renderizando.
3. Só então abrir a Fase 1 completa.

---

## 12. Estado da execução

### Marco 0 — shell + contrato + ponte (concluído)

- Workspace Cargo (`arcadia-store`, `arcadia-contracts`, `arcadia-download`, `arcadia-ipc`, `src-tauri`) compilando com MSVC.
- Toolchain desta máquina: Rust 1.99 MSVC + VS Build Tools 2022 + Windows SDK; `rc.exe` fixado em `.cargo/config.toml`. WebView2 Evergreen 157 instalado.
- `ui/` é a cópia do renderer React; alterações: import da ponte em `main.tsx`, porta 1420 no Vite e CSP com `ipc:`/`ws://localhost:1420`. Nenhuma mudança visual.
- Ponte gerada do preload congelado (`tools/gen-ipc.mjs`): **202 canais invoke + 22 eventos**, com `shim/ipc-manifest.json` e `crates/arcadia-ipc/src/channels.rs` derivados da mesma fonte.
- Teste estático da ponte (`tools/test-bridge.mjs`) executa o bridge gerado contra um mock do Tauri: 202 + 22, invoke/event/unsubscribe provados.
- Dispatcher Rust com `arcadia_ipc`: `library:get`, `config:get/set` (com máscara de segredos), janela, modo, boot vídeo e leituras de boot com estado vazio seguro. Canais ainda não portados respondem `not_ported`.
- Janela Tauri frameless 1600×900 abrindo a UI React real no WebView2, com boot comprovado por screenshot.
- `library:get` e `config:get` comprovados renderer → Tauri → Rust via `ARCADIA_IPC_LOG` (11 canais no boot).
- Instância única (`tauri-plugin-single-instance`): segunda execução sai em ~70 ms e foca a existente.

### Portes puros já verdes

| Crate | Conteúdo | Testes |
|---|---|---|
| `arcadia-contracts` | index.js + sync.js completos, com 1.304 casos diferenciais contra o JS | 43 |
| `arcadia-store` | paths, config, library, overrides, session (DPAPI + fallback), contas, owned | 39 |
| `arcadia-download` | queue policy + integridade + máquina de estados | 25 |
| `arcadia-ipc` | dispatcher, máscara de segredos, cobertura de canais | 6 |
| **Total** | | **113** |

### Próximo marco — Fase 2

- `arcadia-api`: auth (`signup/login/refresh/logout`), REST (`profiles`, `friendships`, `avatars`), RPCs, storage, DoH + IPv4 + fallback de endpoint.
- Sessão restaurada do `session.json` real, `conta` ativa e `library:get` escopado por conta.
- `biblioteca`/`sync`: pull/push com resolução de conflito já portada, primeiro em modo pull-only.
- Smoke de dados: login com conta existente e `GET /auth/v1/user` idêntico ao Electron.

