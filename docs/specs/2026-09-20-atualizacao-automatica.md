# Atualização automática do Arcadia — design

- **Data:** 2026-09-20
- **Status:** desenho aprovado pelo dono — aguardando plano de implementação
- **Escopo:** como o app avisa, baixa e aplica versões novas sem o usuário procurar o release na mão

## Problema

Hoje toda atualização é manual: o usuário precisa perceber que saiu versão nova, abrir o GitHub, escolher o asset certo (AppImage / Setup / portable / zip) e baixar. Os releases já existem com os 4 assets, mas nada no app sabe disso.

## Decisões aprovadas

1. **`electron-updater`** (a lib do próprio electron-builder), **não** um updater caseiro — reescrever o swap do binário é onde instaladores costumam quebrar.
2. **`autoDownload: false`** — o app **pergunta antes de baixar** (economiza banda).
3. **`autoInstallOnAppQuit: false`** — o reinício é explícito, nunca surpresa.
4. **Portable e zip não têm auto-update** (o `.exe` é extraído a cada execução): recebem o **aviso** "baixe a versão nova" com link pro release. AppImage e Setup (NSIS) usam o updater de verdade.
5. **Só quando empacotado** (`app.isPackaged`): rodando da fonte (`./arcadia.sh`) o updater não faz nada — desenvolvimento fica intocado.
6. **Quando checar:** ~30s depois de abrir e a cada 6 horas.
7. **UI:** diálogo curto no padrão do app + uma linha em Configurações.

## Arquitetura

```
boot ──30s──▶ updater.check()
                 │  (autoUpdater.checkForUpdates, autoDownload=false)
                 ▼
        versão nova? ──não──▶ silêncio (só log)
                 │sim
                 ▼
   diálogo "Arcadia X.Y.Z disponível — Baixar / Depois"
                 │ Baixar
                 ▼
        downloadUpdate() ──▶ progresso (por %) ──▶ "Pronto — Reiniciar agora / Depois"
                 │ Reiniciar
                 ▼
        quitAndInstall()
```

Componentes:

| # | Peça | Arquivo | Papel |
|---|---|---|---|
| 1 | Config de publicação | `app/package.json` (`build.publish`) | fazer o build gerar `latest.yml` (win) e `latest-linux.yml` |
| 2 | Dependência | `app/package.json` | `electron-updater` |
| 3 | Módulo do updater | `app/electron/updater.js` (novo) | encapsular check/download/install, eventos e o gating (`isPackaged`, dev) |
| 4 | Wiring | `app/electron/main.js` | instanciar no boot, IPC pro renderer, diálogos |
| 5 | UI | `app/src/components/desktop/*` + `SettingsPanel` | o diálogo de versão nova, o progresso e a linha "Verificar atualizações" |
| 6 | Release | fluxo de publicação (hoje `gh release create` manual) | **anexar os dois `.yml`** junto dos 4 assets |

## Fluxo de dados

- O updater lê a **versão local** de `app.getVersion()` (vem do `package.json` empacotado).
- Compara com o `latest.yml` / `latest-linux.yml` publicado no **release mais recente** do repo público `imperat-on/arcadia` (sem token: repo é público).
- O download vai pro cache do electron-updater; no Linux (AppImage) o arquivo `.AppImage` é substituído no lugar; no Windows (NSIS) o instalador roda no `quitAndInstall()`.
- Nada é escrito em `~/.local/share/arcadia/` — o updater vive no diretório de cache do próprio Electron.

## Erros e casos-limite

| Caso | Comportamento |
|---|---|
| Sem rede / GitHub fora | log e silêncio; tenta de novo no próximo ciclo (6h) |
| Portable / zip | detecta e mostra "Baixe a versão nova" (link do release) |
| Rodando da fonte (`./arcadia.sh`) | updater desligado (`isPackaged === false`) |
| AppImage movido/extraído | o updater falha ao substituir: mostra o aviso com link (mesmo caminho do portable) |
| Windows sem assinatura de código | o updater funciona; o SmartScreen avisa na primeira execução ("editor desconhecido") — limitação aceita |
| Usuário escolhe "Depois" | nada baixa; o aviso reaparece no próximo boot/ciclo |
| Falha no download | mensagem curta e o botão volta a "Baixar" |

## Testes

- Unitário: gating (`isPackaged`), comparação de versão, o "pergunta antes de baixar" (não chama `downloadUpdate` sem resposta).
- Unitário: detecção de canal não-suportado (portable/zip) → caminho do aviso.
- Integração leve: o fluxo com o `autoUpdater` mockado (eventos `update-available`, `download-progress`, `update-downloaded`), sem tocar em rede.
- Manual (documentado no PR): AppImage 1.4.1 → publicar 1.4.2 → ver o diálogo, baixar, reiniciar e conferir a versão.

## Fora de escopo (agora)

- Assinatura de código (exige certificado pago) — fica documentado como limitação.
- Differential/delta downloads.
- Canais beta/stable separados.
- Atualização automática para quem roda da fonte (o `git pull` é o "updater" desse caso).
