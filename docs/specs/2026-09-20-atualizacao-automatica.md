# Atualização automática do Arcadia — design

- **Data:** 2026-09-20 (revisado em 2026-09-21 após review adversarial)
- **Status:** desenho aprovado pelo dono; ajustes do review aplicados — aguardando plano de implementação
- **Escopo:** como o app avisa, baixa e aplica versões novas sem o usuário procurar o release na mão
- **Review que originou os ajustes:** `/tmp/review-design/REVIEW.md` (veredito AJUSTES NECESSÁRIOS, 2 bloqueadores)

## Problema

Hoje toda atualização é manual para quem usa o app empacotado: o usuário precisa perceber que saiu versão nova, abrir o GitHub, escolher o asset certo e baixar. Os releases já existem com os 4 assets, mas o app empacotado não sabe disso.

## Decisões aprovadas

1. **`electron-updater`** (a lib do electron-builder), **não** um updater caseiro.
2. **`autoDownload: false`** — o app **pergunta antes de baixar** (economiza banda).
3. **`autoInstallOnAppQuit: false`** — o reinício é explícito, nunca surpresa.
4. **Portable e zip não têm auto-update** (o `.exe` é extraído a cada execução): recebem o **aviso** "baixe a versão nova" com link. AppImage e Setup (NSIS) usam o updater de verdade.
5. **Só quando empacotado** (`app.isPackaged`).
6. **Quando checar:** ~30s depois de abrir e a cada 6 horas.
7. **UI:** reusar o diálogo existente (ver "UI") + a linha de atualizações que já existe em Configurações.
8. **(novo)** O diálogo é **no renderer**, reusando `UpdateDialog.tsx` — nativo (`dialog.showMessageBox`) não tem gamepad e seria regressão no modo console.
9. **(novo)** A checagem **respeita o toggle `check_updates_on_start`** que já existe.
10. **(novo)** O prompt **é adiado com jogo rodando** (o app já sabe: `jogoRodando`).
11. **(novo)** O estado "já avisei nesta versão" fica em memória + config, para não reabrir o diálogo a cada 6h.

## Convivência dos dois canais de atualização (BLOQUEADOR do review)

**Já existe** um updater em produção: `app/electron/updater.js` (157 linhas) é o updater **git** para quem roda da fonte — `git pull --ff-only` + `npm install` + `npm run build`. Está ligado em:

- `main.js:53` (`require("./updater")`), `main.js:2510-2518` (boot, respeitando `check_updates_on_start`), `main.js:2907-2910` (no `did-finish-load`), `main.js:4118-4142` (IPC `update:state` / `update:check` / `update:apply`)
- `preload.js:256-269`, `global.d.ts:1340-1351`
- `UpdateDialog.tsx` + `DesktopLauncher.tsx:503` + `PS5Launcher.tsx:173,1772`

**Nada disso muda.** O desenho adiciona um **segundo canal**, para o app empacotado:

| Situação | Canal | Módulo | UI |
|---|---|---|---|
| Rodando da fonte (`./arcadia.sh`, `isPackaged === false`) | updater **git** (existente, intocado) | `app/electron/updater.js` | `UpdateDialog` (como hoje) |
| Empacotado (AppImage / NSIS) | **electron-updater** (novo) | `app/electron/updater-packaged.js` (nome distinto, para não confundir) | mesmo `UpdateDialog`, estados novos |
| Empacotado sem suporte (portable / zip) | nenhum updater | — | aviso "baixe a versão nova" com link |

O `main.js` ramifica por `app.isPackaged`. Os **canais IPC do canal empacotado são novos** (`update:packaged:*`) — nunca reusar/sobrescrever `update:state`/`update:check`/`update:apply`, que são do fluxo git.

## Arquitetura

```
boot ──30s──▶ (isPackaged?)
                 ├─ não ─▶ updater git (existente, inalterado)
                 └─ sim ─▶ updater-packaged.check()
                              │  autoUpdater.checkForUpdates()  (autoDownload=false)
                              ▼
                     versão nova? ──não──▶ silêncio (só log)
                              │sim
                              ▼
              UpdateDialog: "Arcadia X.Y.Z disponível — Baixar / Depois"
                              │ Baixar
                              ▼
                   downloadUpdate() ──▶ progresso (%) ──▶ "Pronto — Reiniciar agora / Depois"
                              │ Reiniciar
                              ▼
                        quitAndInstall()
```

| # | Peça | Arquivo | Papel |
|---|---|---|---|
| 1 | `repository` + `build.publish` | `app/package.json` | `publish: { provider: "github", owner: "imperat-on", repo: "arcadia" }`; gera o `app-update.yml` **dentro do pacote** (é o que o electron-updater lê em runtime) e os `latest*.yml` no build |
| 2 | Dependência | `app/package.json` | `electron-updater` em **`dependencies`** (em devDependencies o electron-builder poda e o `require` quebra no empacotado) |
| 3 | Scripts | `app/package.json` | `dist:*` passam a levar **`--publish never`** (sem isso o electron-builder publica sozinho se houver `GH_TOKEN`/tag) |
| 4 | Módulo do updater empacotado | `app/electron/updater-packaged.js` (novo) | encapsular check/download/install, eventos (`error` incluído), gating (`isPackaged`, canal suportado) e o estado "já avisei" |
| 5 | Wiring | `app/electron/main.js` | instanciar no boot (ramo `isPackaged`), IPC `update:packaged:*`, adiar com jogo rodando |
| 6 | UI | `app/src/components/ps5-launcher/UpdateDialog.tsx` (reuso) + `app/src/components/desktop/settings/GeneralSection.tsx` (toggle/botão existentes) | estados novos: "disponível", "baixando (N%)", "pronto" |
| 7 | i18n | `app/src/i18n/{pt-BR,en-US,es-ES}.json` | chaves novas nos 3 (o teste `i18n-sem-duplicatas.test.js:35` exige conjuntos idênticos) |
| 8 | Release | processo de publicação | anexar o `.yml` **do build correspondente** (ver abaixo) |

## Fluxo de release (fechado)

- **NSIS (`dist:nsis`) → `latest.yml`** — é o yml do Windows.
- **AppImage (`dist:appimage`) → `latest-linux.yml`** — é o yml do Linux.
- **`portable` e `zip` não geram yml nenhum** (por design do electron-builder).
- O `release/` acumula yml de builds anteriores: o script de release precisa **usar o yml da versão que está sendo publicada**, nunca o mais antigo que estiver na pasta.
- O release precisa estar **publicado, não draft e não prerelease** — o electron-updater ignora draft/prerelease.
- Hoje não existe workflow de release (só `ci.yml` com test/tsc/build): o release é manual (`gh release create` com os 4 assets). Com este desenho ele passa a anexar **também os 2 yml**.

## Detecção de canal (no app empacotado)

| Canal | Como detectar |
|---|---|
| NSIS (Setup) | `app-update.yml` presente em `resources/` **e** não-portable |
| AppImage | variável de ambiente `APPIMAGE` definida |
| **Portable** | env `PORTABLE_EXECUTABLE_FILE` / `PORTABLE_EXECUTABLE_DIR` (o stub do electron-builder define) |
| **Zip** | **ausência de `resources/app-update.yml`** (só é escrito quando o target NSIS existe) — o mesmo critério cobre qualquer empacotado sem suporte |
| Fonte | `isPackaged === false` |

"Canal não suportado" (portable/zip) **não é erro**: é o caminho do aviso com link para a página do release.

## Erros e casos-limite

| Caso | Comportamento |
|---|---|
| Sem rede / GitHub fora | log e silêncio; tenta de novo no próximo ciclo (6h). **Listener explícito do evento `error`** (sem ele o EventEmitter lança e pode derrubar o processo) |
| Portable / zip | aviso "Baixe a versão nova" + botão pro release |
| Rodando da fonte | canal git de sempre, intocado |
| AppImage **movido/extraído** | falha ao substituir → aviso com link (mesmo caminho do portable) |
| `APPIMAGE` indefinido | é falha **antes** de tentar substituir → aviso com link, nunca silêncio |
| AppImage em local **sem permissão de escrita** (`/opt`, montagem read-only) | falha no `quitAndInstall` → aviso com link e mensagem clara |
| Windows sem assinatura | o updater funciona; SmartScreen/Defender avisa na instalação **e** no update silencioso — limitação aceita e documentada |
| Instalação NSIS por-máquina (`oneClick:false`, `allowToChangeInstallationDirectory:true`) | o update exige UAC; caso a documentar/testar |
| Jogo rodando | o prompt é **adiado** (estado `jogoRodando`); reavalia no próximo ciclo |
| Usuário escolhe "Depois" **antes** de baixar | nada baixa; o aviso automático não reabre para a mesma versão (D7: memória + config `update_ja_avisado`); a checagem manual em Configurações continua mostrando |
| Usuário escolhe "Depois" **depois** de baixar | o update fica pendente no cache (`update_pendente_versao`); o aviso "Pronto — Reiniciar agora" reaparece no próximo boot; o arquivo **completo** em cache é reaproveitado (o electron-updater 6 descarta download parcial — não há retomada byte a byte; no NSIS o differential reduz o download) |
| Falha no download | mensagem curta, o botão volta a "Baixar" |
| Mensagem legada `update.bloqueado.sem-git` | hoje o usuário empacotado que clica em "Procurar atualizações" vê "não é um clone do Git, reinstale" — passa a apontar para o canal empacotado |

## Estado e cache

- O cache do electron-updater **não** é o do Electron: Linux `~/.cache/arcadia-updater`, Windows `%LOCALAPPDATA%\arcadia-updater`.
- O AppImage é substituído **no lugar** (fora do cache).
- "Já avisei nesta versão" → memória + config (para não repetir a cada 6h).

## Decisões fechadas (do review)

| # | Decisão | Resolução |
|---|---|---|
| D1 | Nome/local do módulo novo | `app/electron/updater-packaged.js`; o `updater.js` git fica intocado |
| D2 | Canais IPC | novos `update:packaged:*`; os `update:*` existentes são do fluxo git |
| D3 | Diálogo nativo vs renderer | renderer, reusando `UpdateDialog.tsx` (gamepad nos dois modos) |
| D4 | Conteúdo do aviso + i18n | versão + tamanho quando disponível; chaves novas nos 3 dicionários |
| D5 | Detecção portable/zip | env `PORTABLE_EXECUTABLE_FILE`; zip = sem `app-update.yml` |
| D6 | "Depois" pós-download, cache, retomada | pendente no cache, reavisa no boot, retoma download parcial |
| D7 | "Já avisei nesta versão" | memória + config |
| D8 | Release: qual yml | NSIS → `latest.yml`; AppImage → `latest-linux.yml`; usar o da versão publicada |
| D9 | `--publish never` | adicionado nos `dist:*` |
| D10 | Toggle `check_updates_on_start` | respeitado no canal empacotado |

## Testes

- A suíte é `node --test` puro, sem Electron: o módulo novo deve ser **injetável** (recebe o `autoUpdater`/o `app`/o `dialog` por parâmetro) para testar gating, comparação de versão, detecção de canal e "não baixa sem resposta" sem Electron.
- Unitário: gating (`isPackaged`, canal suportado), "pergunta antes de baixar" (não chama `downloadUpdate` sem resposta), "Depois" antes e depois do download, adiamento com jogo rodando.
- Unitário: detecção de canal (portable por env, zip por ausência do `app-update.yml`, AppImage por `APPIMAGE`).
- Integração leve: fluxo com `autoUpdater` mockado (`update-available`, `download-progress`, `update-downloaded`, `error`), sem rede.
- Manual (documentado): AppImage 1.4.1 → publicar 1.4.2 → ver o diálogo, baixar, reiniciar, conferir a versão.

## Fora de escopo (agora)

- Assinatura de código (exige certificado pago) — limitação documentada.
- Differential/delta downloads.
- Canais beta/stable separados.
- Atualizar quem roda da fonte (o canal git existente já faz isso).
