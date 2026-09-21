# Atualização automática (canal empacotado) — plano de implementação (v2)

> **For agentic workers:** REQUIRED SUB-SKILL: use `subagent-driven-development` (recomendado) ou `executing-plans` para implementar task por task. Passos em checkbox (`- [ ]`).
>
> **v2:** achados do review adversarial (`/tmp/review-plan/REVIEW.md`, veredito AJUSTES NECESSÁRIOS) aplicados: **B1–B5** e **M1–M8**. Cada step de código traz o **código real** que será copiado; sem placeholder e sem "similar à Task N".

**Goal:** o app empacotado (AppImage/NSIS) avisa que existe versão nova, baixa só se o usuário aceitar, e aplica no restart — sem tocar no updater git de quem roda da fonte.

**Architecture:** um módulo novo (`app/electron/updater-packaged.js`) encapsula o `electron-updater` com dependências injetáveis (para testar sem Electron). O `main.js` cria a instância **sempre** no boot (empacotado ou não) e expõe IPC `update:packaged:*`; só o ciclo automático (30s + 6h) é gateado pelo toggle. No renderer, um hook novo (`useAtualizacaoEmpacotada`) assina o push `update:packaged:changed` e monta o `UpdateDialog` existente nos dois launchers. O build passa a gerar `app-update.yml` + `latest*.yml`, e o release passa a anexá-los.

**Tech Stack:** Electron 33, electron-updater 6.x, electron-builder (NSIS/AppImage), React + TypeScript (Vite), `node --test` (sem Electron na suíte).

**Spec:** `docs/specs/2026-09-20-atualizacao-automatica.md` — atualizado neste mesmo commit para remover a contradição D6/D7 da linha 109 (ver Task 2, "Depois antes de baixar") e a promessa de retomada byte a byte (o electron-updater 6 descarta download parcial; o cache reaproveitado é o do arquivo **completo**).

## Rastreio dos achados do review

| Achado | Resolvido em |
|---|---|
| B1 — caminhos errados (`UpdateDialog`, `GeneralSection`) e README sem seção de release | Task 4B/4C (caminhos reais) e Task 5 (`docs/release.md` novo + seção "Releasing" no README) |
| B2 — gatilho automático do diálogo inexistente | Task 4B (`useAtualizacaoEmpacotada`) + Task 4C (launchers `DesktopLauncher.tsx:228,503` e `PS5Launcher.tsx:173,1772`) + Task 3D (push de boot cobre portable/zip) |
| B3 — contrato único do módulo | Task 2 (`temAppUpdateYml`, `onChange`, `fase: "sem_suporte"`, `motivo?`, estado completo documentado, `jaAvisado`/`salvarJaAvisado`/`pendente` no contrato) + Task 3A (config: allowlist `main.js:4236` e `AppConfig` `global.d.ts:250`) |
| B4 — detecção de canal por plataforma | Task 2 (`detectarCanal` com `platform`; Linux sem `APPIMAGE` → `sem_suporte`, nunca `nsis`) + testes 3 |
| B5 — D6 (Depois pós-download/cache/reavisa no boot) | Task 2 (`pendente`/`salvarPendente` + redownload do cache) + testes 9, 14, 15; spec ajustado (sem retomada byte a byte) |
| M1 — tamanho no aviso (D4) | Task 2 (`tamanhoDe`) + Task 4A (`disponivel_tamanho`) + Task 4B (`fmtBytes`) |
| M2 — instância sempre; só o ciclo gateado | Task 3A (instância no `whenReady`) + Task 3D (30s/6h com toggle) |
| M3 — erro de fundo silencioso × erro de ação | Task 2 (`erroDeFundo`) + Task 4B (hook ignora `erroDeFundo`) + teste 10 |
| M4 — renderer sem `isPackaged` | Task 4C (ramifica por `canal !== "fonte"` do IPC novo) |
| M5 — release: seção + bump + assinatura/UAC | Task 5 |
| M6 — cobertura de testes | Task 2, testes 8–16 (jogo rodando, Depois antes/depois, `update-downloaded`, progresso com assert, `quitAndInstall`, comparação de versão, já avisei) |
| M7 — Task 3 atômica | Tasks 3A, 3B, 3C, 3D (uma preocupação cada, commit próprio) |
| M8 — nomes reais (`jogoRodando`, `win`, boot 2910/2959) | Tasks 3A/3D (código com `() => jogoRodando`, `win`, `did-finish-load`) |

## Global Constraints

- **Não tocar** em `app/electron/updater.js` (canal git), nos IPC `update:state`/`update:check`/`update:apply`, nem em `preload.js:256-269` — o canal da fonte fica exatamente como está. A única mudança no fluxo git é a ramificação explícita `if (app.isPackaged) return` em `procurarAtualizacao` (Task 3D).
- Nomes novos: módulo `app/electron/updater-packaged.js`; canais IPC `update:packaged:state|check|download|install|jaAvisado`; push `update:packaged:changed`; chaves de config `update_ja_avisado` e `update_pendente_versao`.
- `electron-updater` vai em **`dependencies`** (nunca `devDependencies`), fixado em `^6.6.2` (a v7/builder 27 muda a API: `autoInstallOnAppQuit` vira shim e `downloadUpdate()` devolve objeto).
- `autoDownload: false` e `autoInstallOnAppQuit: false` são invariantes do desenho.
- Todo texto de UI novo entra nos **3** dicionários (`pt-BR`, `en-US`, `es-ES`) — o teste `i18n-sem-duplicatas.test.js:35` exige conjuntos idênticos.
- A suíte é `node --test` puro: nada de `import { app } from "electron"` em código de teste.
- Textos curtos e diretos (padrão do dono).
- Verificação por task: `npx tsc --noEmit`, `npm test` (esperado: 1 falha pré-existente, `steam-path`), `npm run build`.

## Ordem e dependências

```
1 (build/deps) ─▶ 2 (módulo) ─▶ 3A (instância/config) ─▶ 3B (IPC) ─▶ 3C (preload/tipos) ─▶ 3D (agendamento/boot)
                                                                          │
                                                          4A (i18n) ─▶ 4B (diálogo/hook) ─▶ 4C (launchers/settings)
                                                                          │
                                                          5 (release) ─▶ 6 (validação E2E)
```

---

### Task 1: Build e publicação (o updater precisa do cardápio)

**Files:**
- Modify: `app/package.json` (bloco `build`, `dependencies`, `scripts.dist:*`)

**Interfaces:**
- Produces: `build.publish` (provider github), `repository`, `electron-updater@^6.6.2` disponível para `require`; os builds passam a escrever `resources/app-update.yml` e `release/latest*.yml`.

- [ ] **Step 1: Limpar yml velho e adicionar `repository` + `publish` ao package.json**

O `app/release/` acumula yml de builds anteriores (1.2.3 a 1.4.1). Antes de qualquer coisa:

Run: `rm -f app/release/latest*.yml`

Em `app/package.json`, logo após `"version": "1.4.1",`, adicione:

```json
  "repository": { "type": "git", "url": "https://github.com/imperat-on/arcadia.git" },
```

Dentro de `"build"`, logo após `"productName": "Arcadia",`, acrescente:

```json
    "publish": { "provider": "github", "owner": "imperat-on", "repo": "arcadia" },
```

- [ ] **Step 2: Instalar o electron-updater como dependência de runtime (major 6)**

Run: `cd app && npm install electron-updater@^6.6.2 --save`

Expected: entra em `"dependencies"`. Confira:

Run: `cd app && python3 -c "import json;print(json.load(open('package.json'))['dependencies']['electron-updater'])"`

Expected: uma versão `^6.x`.

- [ ] **Step 3: Blindar os scripts contra publish implícito**

Troque os 4 scripts (hoje em `app/package.json:76-79`, sem `--publish never`):

```json
    "dist:appimage": "vite build && electron-builder --linux AppImage --x64 --publish never",
    "dist:nsis": "vite build && electron-builder --win nsis --x64 --publish never",
    "dist:portable": "vite build && electron-builder --win portable --x64 --publish never",
    "dist:zip": "vite build && electron-builder --win zip --x64 --publish never",
```

- [ ] **Step 4: Provar que o build gera o cardápio (Windows)**

Run: `cd app && npm run dist:nsis 2>&1 | tail -5 && ls -la release/latest.yml release/win-unpacked/resources/app-update.yml`

Expected: `latest.yml` na raiz de `release/` e `app-update.yml` dentro de `resources/` do pacote.

- [ ] **Step 5: Provar o yml do Linux e o `app-update.yml` do pacote Linux**

Run: `cd app && npm run dist:appimage 2>&1 | tail -5 && ls -la release/latest-linux.yml release/linux-unpacked/resources/app-update.yml`

Expected: `latest-linux.yml` presente **e** `linux-unpacked/resources/app-update.yml` presente (é o que o `AppImageUpdater` lê em runtime). Nota: `portable` e `zip` NÃO geram yml — comportamento esperado.

- [ ] **Step 6: Conferir que os yml são da versão atual (não sobraram dos antigos)**

Run: `cd app && grep -H '^version:' release/latest.yml release/latest-linux.yml`

Expected: as duas linhas com a versão de `app/package.json` (hoje `1.4.1`).

- [ ] **Step 7: Commit**

```bash
git add app/package.json app/package-lock.json
git commit -m "build: publish github + electron-updater 6 em dependencies + --publish never"
```

---

### Task 2: O módulo do updater empacotado (injetável e testável)

**Files:**
- Create: `app/electron/updater-packaged.js`
- Test: `app/test/updater-packaged.test.js`

**Interfaces:**
- Consumes: nada das outras tasks.
- Produces (contrato único — o mesmo da Task 3 e da Task 4; **nada** fora daqui):
  - `createPackagedUpdater({ app, autoUpdater, env, temAppUpdateYml, platform, isJogoRodando, jaAvisado, salvarJaAvisado, pendente, salvarPendente, onChange })` → `{ canal(), suportado(), estado(), checar(), baixar(), instalar(), marcarJaAvisado() }`
  - `estado()` → `{ canal: "fonte"|"appimage"|"nsis"|"portable"|"zip"|"sem_suporte", suportado: boolean, versaoAtual: string, versaoNova: string|null, tamanho: number|null, fase: "ocioso"|"disponivel"|"baixando"|"pronto"|"erro"|"sem_suporte", progresso: number, erro: string|null, erroDeFundo: boolean, jaAvisado: boolean, jogoRodando: boolean }`
  - `checar({ manual = false } = {})` → `Promise<{ ok: boolean, disponivel: boolean, versao?: string|null, tamanho?: number|null, motivo?: "canal_nao_suportado"|"jogo_rodando"|"erro", erro?: string }>` (NUNCA baixa sozinho)
  - `baixar()` → `Promise<{ ok: boolean, erro?: string }>` (só é chamado depois do "Baixar" do usuário)
  - `instalar()` → `{ ok: boolean, erro?: string }` (chama `quitAndInstall`)
  - `marcarJaAvisado(versao)` → `{ ok: boolean }` (persiste via `salvarJaAvisado`)
  - `onChange(estado)` é chamado a cada mudança — o main só repassa para a janela.

- [ ] **Step 1: Escrever o teste que falha**

Crie `app/test/updater-packaged.test.js` exatamente com este conteúdo (17 casos cobrindo B4, D4, D6, D7, M3, M6, M8):

```js
import test from "node:test"
import assert from "node:assert/strict"
import { createRequire } from "node:module"

const require = createRequire(import.meta.url)
const { createPackagedUpdater } = require("../electron/updater-packaged.js")

const fakeUpdater = () => {
  const listeners = {}
  const up = {
    autoDownload: true,
    autoInstallOnAppQuit: true,
    instalou: false,
    chamouCheck: 0,
    baixou: 0,
    on: (ev, fn) => { (listeners[ev] ||= []).push(fn) },
    emitir: (ev, arg) => (listeners[ev] || []).forEach((fn) => fn(arg)),
    checkForUpdates: async () => {
      up.chamouCheck++
      return { updateInfo: { version: "9.9.9", files: [{ size: 104857600 }] } }
    },
    downloadUpdate: async () => { up.baixou++ },
    quitAndInstall: () => { up.instalou = true },
  }
  return up
}

const base = (over = {}) => ({
  app: { isPackaged: true, getVersion: () => "1.4.1" },
  autoUpdater: fakeUpdater(),
  env: {},
  temAppUpdateYml: true,
  platform: "win32",
  ...over,
})

test("canal: fonte quando nao empacotado", () => {
  const u = createPackagedUpdater(base({ app: { isPackaged: false, getVersion: () => "1.4.1" } }))
  assert.equal(u.canal(), "fonte")
  assert.equal(u.suportado(), false)
  assert.equal(u.estado().fase, "ocioso")
})

test("canal: appimage pela env APPIMAGE (linux)", () => {
  const u = createPackagedUpdater(base({ platform: "linux", env: { APPIMAGE: "/tmp/Arcadia.AppImage" } }))
  assert.equal(u.canal(), "appimage")
  assert.equal(u.suportado(), true)
})

test("canal: linux sem APPIMAGE nunca vira nsis (extraido/movido) (B4)", () => {
  const u = createPackagedUpdater(base({ platform: "linux", temAppUpdateYml: true }))
  assert.equal(u.canal(), "sem_suporte")
  assert.equal(u.suportado(), false)
  assert.equal(u.estado().fase, "sem_suporte")
})

test("canal: portable pela env PORTABLE_EXECUTABLE_FILE (win32)", () => {
  const u = createPackagedUpdater(base({ env: { PORTABLE_EXECUTABLE_FILE: "C:\\\\Arcadia.exe" } }))
  assert.equal(u.canal(), "portable")
  assert.equal(u.suportado(), false)
})

test("canal: zip quando falta o app-update.yml (win32)", () => {
  const u = createPackagedUpdater(base({ temAppUpdateYml: false }))
  assert.equal(u.canal(), "zip")
  assert.equal(u.suportado(), false)
})

test("canal: nsis com app-update.yml (win32)", () => {
  const u = createPackagedUpdater(base())
  assert.equal(u.canal(), "nsis")
  assert.equal(u.suportado(), true)
})

test("checar NAO baixa sozinho, extrai o tamanho e desliga o autoDownload (D4)", async () => {
  const up = fakeUpdater()
  const u = createPackagedUpdater(base({ autoUpdater: up }))
  const r = await u.checar()
  assert.equal(r.ok, true)
  assert.equal(r.disponivel, true)
  assert.equal(r.versao, "9.9.9")
  assert.equal(r.tamanho, 104857600)
  assert.equal(up.autoDownload, false)
  assert.equal(up.autoInstallOnAppQuit, false)
  assert.equal(up.baixou, 0, "nao pode baixar antes do usuario aceitar")
  assert.equal(u.estado().fase, "disponivel")
  assert.equal(u.estado().tamanho, 104857600)
})

test("baixar so roda quando chamado e emite progresso de verdade (M6)", async () => {
  const up = fakeUpdater()
  const vistos = []
  const u = createPackagedUpdater(base({ autoUpdater: up, onChange: (e) => vistos.push(e) }))
  await u.checar()
  assert.equal(up.baixou, 0)
  up.downloadUpdate = async () => {
    up.baixou++
    up.emitir("download-progress", { percent: 42.6 })
    up.emitir("update-downloaded", { version: "9.9.9" })
  }
  const r = await u.baixar()
  assert.equal(r.ok, true)
  assert.equal(up.baixou, 1)
  assert.ok(
    vistos.some((e) => e.fase === "baixando" && e.progresso === 43),
    "download-progress tem que aparecer no estado com o percentual arredondado",
  )
  assert.equal(u.estado().fase, "pronto")
  assert.equal(u.estado().progresso, 100)
})

test("update-downloaded deixa pronto e persiste o pendente (D6)", () => {
  const up = fakeUpdater()
  const salvos = []
  const u = createPackagedUpdater(base({ autoUpdater: up, salvarPendente: (v) => salvos.push(v) }))
  up.emitir("update-downloaded", { version: "9.9.9" })
  assert.equal(u.estado().fase, "pronto")
  assert.equal(u.estado().versaoNova, "9.9.9")
  assert.deepEqual(salvos, ["9.9.9"])
})

test("erro de fundo nao abre dialogo; erro manual abre (M3)", async () => {
  const up = fakeUpdater()
  up.checkForUpdates = async () => { throw new Error("sem rede") }
  const u = createPackagedUpdater(base({ autoUpdater: up }))
  const r = await u.checar()
  assert.equal(r.ok, false)
  assert.equal(u.estado().fase, "erro")
  assert.match(String(u.estado().erro), /sem rede/)
  assert.equal(u.estado().erroDeFundo, true)
  const r2 = await u.checar({ manual: true })
  assert.equal(r2.ok, false)
  assert.equal(u.estado().erroDeFundo, false)
  up.emitir("error", new Error("boom"))
  assert.equal(u.estado().erroDeFundo, true, "evento error fora de acao e' de fundo")
})

test("jogo rodando adia a checagem automatica; manual ignora (M6/M8)", async () => {
  const up = fakeUpdater()
  const u = createPackagedUpdater(base({ autoUpdater: up, isJogoRodando: () => true }))
  const r = await u.checar()
  assert.equal(r.ok, true)
  assert.equal(r.disponivel, false)
  assert.equal(r.motivo, "jogo_rodando")
  assert.equal(up.chamouCheck, 0)
  assert.equal(u.estado().jogoRodando, true)
  const r2 = await u.checar({ manual: true })
  assert.equal(r2.disponivel, true)
  assert.equal(up.chamouCheck, 1)
})

test("comparacao de versao e numerica, nao lexicografica (M6)", async () => {
  const up = fakeUpdater()
  up.checkForUpdates = async () => ({ updateInfo: { version: "1.4.10", files: [] } })
  const u = createPackagedUpdater(
    base({ autoUpdater: up, app: { isPackaged: true, getVersion: () => "1.4.9" } }),
  )
  assert.equal((await u.checar()).disponivel, true)
  up.checkForUpdates = async () => ({ updateInfo: { version: "1.4.9", files: [] } })
  const u2 = createPackagedUpdater(
    base({ autoUpdater: up, app: { isPackaged: true, getVersion: () => "1.4.9" } }),
  )
  assert.equal((await u2.checar()).disponivel, false)
})

test("ja avisei le do config e a checagem manual reabre (D7/M6)", async () => {
  const up = fakeUpdater()
  const salvos = []
  const u = createPackagedUpdater(
    base({
      autoUpdater: up,
      jaAvisado: (v) => v === "9.9.9",
      salvarJaAvisado: (v) => salvos.push(v),
    }),
  )
  await u.checar()
  assert.equal(u.estado().jaAvisado, true)
  assert.equal(u.marcarJaAvisado("9.9.9").ok, true)
  assert.deepEqual(salvos, ["9.9.9"])
  await u.checar({ manual: true })
  assert.equal(u.estado().jaAvisado, false, "manual mostra mesmo com 'ja avisei'")
})

test("pendente no boot reaproveita o cache e fica pronto (D6)", async () => {
  const up = fakeUpdater()
  up.downloadUpdate = async () => {
    up.baixou++
    up.emitir("update-downloaded", { version: "9.9.9" })
  }
  const u = createPackagedUpdater(base({ autoUpdater: up, pendente: () => "9.9.9" }))
  const r = await u.checar()
  assert.equal(r.disponivel, true)
  assert.equal(up.baixou, 1, "o cache e' revalidado pelo electron-updater, sem nova pergunta")
  assert.equal(u.estado().fase, "pronto")
})

test("update-not-available limpa o pendente (D6)", async () => {
  const up = fakeUpdater()
  const limpou = []
  up.checkForUpdates = async () => {
    up.emitir("update-not-available", { version: "1.4.1" })
    return { updateInfo: { version: "1.4.1", files: [] } }
  }
  const u = createPackagedUpdater(base({ autoUpdater: up, salvarPendente: (v) => limpou.push(v) }))
  const r = await u.checar()
  assert.equal(r.disponivel, false)
  assert.equal(u.estado().fase, "ocioso")
  assert.deepEqual(limpou, [null])
})

test("instalar so depois do pronto e chama quitAndInstall (M6)", async () => {
  const up = fakeUpdater()
  const u = createPackagedUpdater(base({ autoUpdater: up }))
  assert.deepEqual(u.instalar(), { ok: false, erro: "sem_download" })
  await u.checar()
  await u.baixar()
  assert.equal(u.estado().fase, "pronto")
  assert.deepEqual(u.instalar(), { ok: true })
  assert.equal(up.instalou, true)
})

test("canal nao suportado nao checa e nao quebra", async () => {
  const up = fakeUpdater()
  const u = createPackagedUpdater(base({ autoUpdater: up, env: { PORTABLE_EXECUTABLE_FILE: "x" } }))
  const r = await u.checar()
  assert.equal(r.ok, false)
  assert.equal(r.disponivel, false)
  assert.equal(r.motivo, "canal_nao_suportado")
  assert.equal(u.estado().fase, "sem_suporte")
  assert.equal(up.chamouCheck, 0)
})
```

- [ ] **Step 2: Rodar e ver falhar**

Run: `cd app && node --test test/updater-packaged.test.js`

Expected: FAIL — `Cannot find module '../electron/updater-packaged.js'`.

- [ ] **Step 3: Implementar o módulo**

Crie `app/electron/updater-packaged.js` exatamente com este conteúdo:

```js
// Atualização do app EMPACOTADO (AppImage/NSIS), via electron-updater.
//
// O updater git (./updater.js) continua sendo o canal de quem roda da fonte.
// Este é o segundo canal: pergunta antes de baixar (autoDownload=false),
// instala só no "Reiniciar agora" (autoInstallOnAppQuit=false) e nunca deixa
// o EventEmitter do autoUpdater derrubar o processo (listener de `error`).
//
// A suíte roda sem Electron: tudo que toca o Electron entra por injeção.

const CANAIS_SUPORTADOS = new Set(["nsis", "appimage"])

/** Compara versões ("v1.4.10" > "1.4.9" — numérico, não lexicográfico). */
function compararVersao(a, b) {
  const partes = (v) =>
    String(v || "")
      .replace(/^v/i, "")
      .split(".")
      .map((n) => parseInt(n, 10) || 0)
  const pa = partes(a)
  const pb = partes(b)
  for (let i = 0; i < Math.max(pa.length, pb.length); i++) {
    const d = (pa[i] || 0) - (pb[i] || 0)
    if (d !== 0) return d > 0 ? 1 : -1
  }
  return 0
}

/** Bytes do arquivo oferecido: o yml publica `files[].size`. */
function tamanhoDe(info) {
  const arquivos = Array.isArray(info?.files) ? info.files : []
  let total = 0
  for (const f of arquivos) {
    const n = Number(f?.size)
    if (Number.isFinite(n) && n > 0) total += n
  }
  return total > 0 ? total : null
}

/**
 * Canal do app empacotado, POR PLATAFORMA (B4):
 * - Linux: `APPIMAGE` é o que define AppImage; sem ela o pacote foi extraído
 *   ou movido e não há como se substituir → `sem_suporte` (aviso com link),
 *   NUNCA `nsis` (o Linux sempre escreve app-update.yml).
 * - Windows: portable pelo env do stub; `zip` quando falta o app-update.yml;
 *   `nsis` quando ele existe.
 * - `fonte`: não empacotado.
 */
function detectarCanal({ app, env, temAppUpdateYml, platform }) {
  if (!app.isPackaged) return "fonte"
  if (platform === "linux") return env.APPIMAGE ? "appimage" : "sem_suporte"
  if (platform === "win32") {
    if (env.PORTABLE_EXECUTABLE_FILE || env.PORTABLE_EXECUTABLE_DIR) return "portable"
    return temAppUpdateYml ? "nsis" : "zip"
  }
  return "sem_suporte"
}

/**
 * Contrato único (B3):
 *   createPackagedUpdater({
 *     app, autoUpdater, env, temAppUpdateYml, platform,
 *     isJogoRodando, jaAvisado, salvarJaAvisado, pendente, salvarPendente, onChange,
 *   }) -> { canal(), suportado(), estado(), checar(), baixar(), instalar(), marcarJaAvisado() }
 *
 * estado(): { canal, suportado, versaoAtual, versaoNova, tamanho, fase,
 *             progresso, erro, erroDeFundo, jaAvisado, jogoRodando }
 * fases: ocioso | disponivel | baixando | pronto | erro | sem_suporte
 * checar({ manual }) -> { ok, disponivel, versao?, tamanho?, motivo?, erro? }
 * baixar() -> { ok, erro? }   instalar() -> { ok, erro? }
 *
 * `onChange(estado)` é chamado a cada mudança; o main só repassa para a janela.
 */
function createPackagedUpdater({
  app,
  autoUpdater = null,
  env = {},
  temAppUpdateYml = false,
  platform = process.platform,
  isJogoRodando = () => false,
  jaAvisado = () => false,
  salvarJaAvisado = () => {},
  pendente = () => null,
  salvarPendente = () => {},
  onChange = () => {},
}) {
  const canal = detectarCanal({ app, env, temAppUpdateYml, platform })
  const suportado = CANAIS_SUPORTADOS.has(canal)

  const estadoAtual = {
    canal,
    suportado,
    versaoAtual: String(app?.getVersion?.() || ""),
    versaoNova: null,
    tamanho: null,
    // `fonte` não é "sem suporte": o canal git cuida dele. `sem_suporte` é o
    // aviso com link do portable/zip/AppImage extraído.
    fase: suportado || canal === "fonte" ? "ocioso" : "sem_suporte",
    progresso: 0,
    erro: null,
    erroDeFundo: false,
    jaAvisado: false,
    jogoRodando: false,
  }

  // Separa erro de fundo (checagem automática → silêncio) de erro de ação do
  // usuário (checagem manual/baixar → diálogo).
  let emAcao = false

  const snapshot = () => ({ ...estadoAtual, jogoRodando: Boolean(isJogoRodando()) })
  const publicar = () => {
    try {
      onChange(snapshot())
    } catch {}
  }
  const mudar = (patch) => {
    Object.assign(estadoAtual, patch)
    publicar()
  }
  const mensagem = (e) => String(e?.message || e || "erro")

  if (suportado && autoUpdater) {
    autoUpdater.autoDownload = false
    autoUpdater.autoInstallOnAppQuit = false

    autoUpdater.on("update-available", (info) => {
      const versao = String(info?.version || "")
      mudar({
        fase: "disponivel",
        versaoNova: versao,
        tamanho: tamanhoDe(info),
        progresso: 0,
        erro: null,
        erroDeFundo: false,
        jaAvisado: Boolean(jaAvisado(versao)),
      })
    })

    autoUpdater.on("download-progress", (p) => {
      mudar({
        fase: "baixando",
        progresso: Math.max(0, Math.min(100, Math.round(Number(p?.percent) || 0))),
        erro: null,
      })
    })

    autoUpdater.on("update-downloaded", (info) => {
      const versao = String(info?.version || estadoAtual.versaoNova || "")
      if (versao) {
        try {
          salvarPendente(versao)
        } catch {}
      }
      mudar({
        fase: "pronto",
        versaoNova: versao || estadoAtual.versaoNova,
        progresso: 100,
        erro: null,
        erroDeFundo: false,
      })
    })

    autoUpdater.on("update-not-available", () => {
      try {
        salvarPendente(null)
      } catch {}
      mudar({
        fase: "ocioso",
        versaoNova: null,
        tamanho: null,
        progresso: 0,
        erro: null,
        erroDeFundo: false,
        jaAvisado: false,
      })
    })

    // Sem este listener o EventEmitter lança e pode derrubar o processo. O
    // handler NUNCA re-lança: só registra o estado (erro de fundo não abre UI).
    autoUpdater.on("error", (e) => {
      mudar({ fase: "erro", erro: mensagem(e), erroDeFundo: !emAcao })
    })
  }

  async function checar({ manual = false } = {}) {
    if (!suportado) {
      mudar({ fase: "sem_suporte" })
      return { ok: false, disponivel: false, motivo: "canal_nao_suportado" }
    }
    // Ciclo automático não interrompe partida; reavalia no próximo ciclo.
    if (!manual && isJogoRodando()) {
      mudar({ jogoRodando: true })
      return { ok: true, disponivel: false, motivo: "jogo_rodando" }
    }

    emAcao = manual
    try {
      const r = await autoUpdater.checkForUpdates()
      const info = r?.updateInfo || null
      const versao = String(info?.version || "")
      const disponivel = Boolean(versao) && compararVersao(versao, estadoAtual.versaoAtual) > 0

      if (!disponivel) {
        mudar({
          fase: "ocioso",
          versaoNova: null,
          tamanho: null,
          progresso: 0,
          erro: null,
          erroDeFundo: false,
          jaAvisado: false,
        })
        return { ok: true, disponivel: false, versao: null }
      }

      // D6: já baixado numa sessão anterior. O electron-updater valida o cache
      // por sha512 e reemite `update-downloaded` sem baixar de novo; se o cache
      // sumiu, baixa a MESMA versão que o usuário já aceitou.
      if (pendente() === versao) {
        mudar({ fase: "baixando", progresso: 0, erro: null, erroDeFundo: false })
        await autoUpdater.downloadUpdate()
        if (estadoAtual.fase !== "pronto") mudar({ fase: "pronto", progresso: 100 })
        return { ok: true, disponivel: true, versao, tamanho: tamanhoDe(info) }
      }

      if (manual) {
        // Checagem manual mostra mesmo com "já avisei" — foi o usuário quem pediu.
        mudar({ fase: "disponivel", versaoNova: versao, tamanho: tamanhoDe(info), jaAvisado: false })
      } else if (estadoAtual.fase !== "disponivel" || estadoAtual.versaoNova !== versao) {
        mudar({
          fase: "disponivel",
          versaoNova: versao,
          tamanho: tamanhoDe(info),
          jaAvisado: Boolean(jaAvisado(versao)),
        })
      }
      return { ok: true, disponivel: true, versao, tamanho: tamanhoDe(info) }
    } catch (e) {
      mudar({ fase: "erro", erro: mensagem(e), erroDeFundo: !manual })
      return { ok: false, disponivel: false, erro: mensagem(e), motivo: "erro" }
    } finally {
      emAcao = false
    }
  }

  async function baixar() {
    if (!suportado) return { ok: false, erro: "canal_nao_suportado" }
    if (!estadoAtual.versaoNova) return { ok: false, erro: "sem_versao" }
    emAcao = true
    mudar({ fase: "baixando", progresso: 0, erro: null, erroDeFundo: false })
    try {
      await autoUpdater.downloadUpdate()
      if (estadoAtual.fase !== "pronto") mudar({ fase: "pronto", progresso: 100 })
      return { ok: true }
    } catch (e) {
      mudar({ fase: "erro", erro: mensagem(e), erroDeFundo: false })
      return { ok: false, erro: mensagem(e) }
    } finally {
      emAcao = false
    }
  }

  function instalar() {
    if (!suportado) return { ok: false, erro: "canal_nao_suportado" }
    if (estadoAtual.fase !== "pronto") return { ok: false, erro: "sem_download" }
    try {
      autoUpdater.quitAndInstall()
      return { ok: true }
    } catch (e) {
      mudar({ fase: "erro", erro: mensagem(e), erroDeFundo: false })
      return { ok: false, erro: mensagem(e) }
    }
  }

  function marcarJaAvisado(versao) {
    const v = String(versao || estadoAtual.versaoNova || "")
    if (!v) return { ok: false }
    try {
      salvarJaAvisado(v)
    } catch {}
    if (estadoAtual.versaoNova === v) mudar({ jaAvisado: true })
    return { ok: true }
  }

  return {
    canal: () => canal,
    suportado: () => suportado,
    estado: () => snapshot(),
    checar,
    baixar,
    instalar,
    marcarJaAvisado,
  }
}

module.exports = { createPackagedUpdater, compararVersao, tamanhoDe, detectarCanal }
```

- [ ] **Step 4: Rodar e ver passar**

Run: `cd app && node --test test/updater-packaged.test.js`

Expected: PASS (17 testes).

- [ ] **Step 5: Suíte e tipos**

Run: `cd app && npx tsc --noEmit && npm test 2>&1 | tail -5`

Expected: tsc limpo; suíte com a única falha pré-existente (`steam-path`).

- [ ] **Step 6: Ajustar o spec (D6/D7 sem contradição)**

Em `docs/specs/2026-09-20-atualizacao-automatica.md`, troque as duas linhas dos casos-limite:

De:
```
| Usuário escolhe "Depois" **antes** de baixar | nada baixa; reavisa no próximo boot/ciclo |
| Usuário escolhe "Depois" **depois** de baixar | o update fica pendente no cache; o aviso "Pronto — Reiniciar agora" reaparece no próximo boot; o arquivo em cache é reaproveitado (download parcial retoma) |
```

Para:
```
| Usuário escolhe "Depois" **antes** de baixar | nada baixa; o aviso automático não reabre para a mesma versão (D7: memória + config `update_ja_avisado`); a checagem manual em Configurações continua mostrando |
| Usuário escolhe "Depois" **depois** de baixar | o update fica pendente no cache (`update_pendente_versao`); o aviso "Pronto — Reiniciar agora" reaparece no próximo boot; o arquivo **completo** em cache é reaproveitado (o electron-updater 6 descarta download parcial — não há retomada byte a byte; no NSIS o differential reduz o download) |
```

- [ ] **Step 7: Commit**

```bash
git add app/electron/updater-packaged.js app/test/updater-packaged.test.js docs/specs/2026-09-20-atualizacao-automatica.md
git commit -m "feat(updater): modulo do canal empacotado (electron-updater, pergunta antes de baixar)"
```

---

### Task 3A: Instância no boot + config (o toggle não desliga o botão manual)

**Files:**
- Modify: `app/electron/main.js` (declarações perto de `let win`, linha 254; instância no `whenReady`, depois de `process.env.ARCADIA_MODE`, linha ~2970)
- Modify: `app/src/global.d.ts` (`AppConfig`, linha ~250)

**Interfaces:**
- Consumes: `createPackagedUpdater` da Task 2 (contrato exato acima).
- Produces: `atualizadorEmpacotado` no escopo do módulo (a Task 3B registra os IPC; a Task 3D agenda o ciclo). Config: `update_ja_avisado` e `update_pendente_versao` no `AppConfig` + allowlist.

- [ ] **Step 1: Declarar a instância, o fallback e o timer no escopo do módulo**

Em `main.js`, logo depois de `let win` (linha 254), adicione:

```js
// Canal empacotado (electron-updater). A instância nasce SEMPRE no boot (o
// botão manual e o aviso portable/zip não dependem do toggle); só o ciclo
// automático de 30s/6h é gateado por check_updates_on_start.
const ESTADO_EMPACOTADO_FONTE = {
  canal: "fonte",
  suportado: false,
  versaoAtual: "",
  versaoNova: null,
  tamanho: null,
  fase: "ocioso",
  progresso: 0,
  erro: null,
  erroDeFundo: false,
  jaAvisado: false,
  jogoRodando: false,
}
let atualizadorEmpacotado = null
let cicloEmpacotado = null
```

- [ ] **Step 2: Tipar as chaves de config novas (`AppConfig`)**

Em `app/src/global.d.ts`, no bloco de Config. Gerais (linha ~250, junto de `check_updates_on_start`), adicione:

```ts
  check_updates_on_start?: boolean
  /** Versão do app empacotado sobre a qual o usuário já escolheu "Depois" (D7). */
  update_ja_avisado?: string
  /** Versão já baixada e pendente de instalação no cache do electron-updater (D6). */
  update_pendente_versao?: string
```

- [ ] **Step 3: Liberar `update_ja_avisado` no allowlist do `config:set`**

Em `main.js:4236`, na linha do `check_updates_on_start` do `ALLOWED_CONFIG`, adicione a chave nova:

```js
        "check_updates_on_start", "start_in_console_mode", "hide_changelog_on_start",
        "update_ja_avisado",
```

(`update_pendente_versao` é escrito só pelo main, via `salvarPendente` — de propósito fora do allowlist.)

- [ ] **Step 4: Criar a instância no `whenReady` (antes do `createWindow`)**

Em `main.js`, no `app.whenReady().then(() => {` (linha 2959), logo depois de `process.env.ARCADIA_MODE = resolveLauncherMode(process.env, readConfig())` e `configurarLojaSteam()`, adicione:

```js
  // Updater do app empacotado: a instância existe SEMPRE (mesmo com o toggle
  // desligado). O canal git continua intocado — este é um segundo canal.
  try {
    const { createPackagedUpdater } = require("./updater-packaged")
    atualizadorEmpacotado = createPackagedUpdater({
      app,
      autoUpdater: app.isPackaged ? require("electron-updater").autoUpdater : null,
      env: process.env,
      temAppUpdateYml: fs.existsSync(path.join(process.resourcesPath, "app-update.yml")),
      isJogoRodando: () => jogoRodando,
      jaAvisado: (versao) => readConfig().update_ja_avisado === versao,
      salvarJaAvisado: (versao) => writeConfig({ update_ja_avisado: versao }),
      pendente: () => readConfig().update_pendente_versao || null,
      salvarPendente: (versao) =>
        versao
          ? writeConfig({ update_pendente_versao: versao })
          : writeConfig({}, ["update_pendente_versao"]),
      onChange: (estado) => {
        if (win && !win.isDestroyed()) win.webContents.send("update:packaged:changed", estado)
      },
    })
  } catch (e) {
    console.error("[updater] canal empacotado indisponível:", e)
  }
```

- [ ] **Step 5: Verificar**

Run: `cd app && npx tsc --noEmit && npm test 2>&1 | tail -5`

Expected: tsc limpo; suíte com a falha pré-existente.

- [ ] **Step 6: Commit**

```bash
git add app/electron/main.js app/src/global.d.ts
git commit -m "feat(updater): instancia do canal empacotado no boot + chaves de config (ja avisei/pendente)"
```

---

### Task 3B: Os IPC novos e o push de estado

**Files:**
- Modify: `app/electron/main.js` (perto dos IPC `update:*`, 4118-4142)

**Interfaces:**
- Consumes: `atualizadorEmpacotado` e `ESTADO_EMPACOTADO_FONTE` da Task 3A.
- Produces: `update:packaged:state|check|download|install|jaAvisado` (invoke) e `update:packaged:changed` (push).

- [ ] **Step 1: Registrar os handlers**

Em `main.js`, logo depois do bloco `ipcMain.handle("update:apply", ...)` (termina por volta da linha 4142), adicione:

```js
  // Canal empacotado (electron-updater) — separado dos update:* do git.
  ipcMain.handle("update:packaged:state", () => atualizadorEmpacotado?.estado() || ESTADO_EMPACOTADO_FONTE)
  ipcMain.handle(
    "update:packaged:check",
    async (_e, { manual = false } = {}) =>
      atualizadorEmpacotado?.checar({ manual }) || { ok: false, disponivel: false, motivo: "fonte" },
  )
  ipcMain.handle(
    "update:packaged:download",
    async () => atualizadorEmpacotado?.baixar() || { ok: false, erro: "fonte" },
  )
  ipcMain.handle("update:packaged:install", () => atualizadorEmpacotado?.instalar() || { ok: false, erro: "fonte" })
  ipcMain.handle(
    "update:packaged:jaAvisado",
    (_e, { versao } = {}) => atualizadorEmpacotado?.marcarJaAvisado(versao) || { ok: false },
  )
```

- [ ] **Step 2: Verificar que o push já está ligado**

O repasse `update:packaged:changed` já foi definido no `onChange` da Task 3A (`main.js`, instância no `whenReady`): o módulo chama `onChange(snapshot)` a cada mudança e o main faz `win.webContents.send(...)`. Confira com:

Run: `cd app && grep -n "update:packaged:changed" electron/main.js`

Expected: 1 ocorrência (dentro do `onChange`).

- [ ] **Step 3: Verificar**

Run: `cd app && npx tsc --noEmit && npm test 2>&1 | tail -5`

Expected: tsc limpo; suíte com a falha pré-existente.

- [ ] **Step 4: Commit**

```bash
git add app/electron/main.js
git commit -m "feat(updater): IPC update:packaged:* no main (fallback documentado)"
```

---

### Task 3C: Expor no preload e tipar no renderer

**Files:**
- Modify: `app/electron/preload.js` (depois de `onUpdateProgress`, linha 269)
- Modify: `app/src/global.d.ts` (tipos novos depois de `UpdateEtapa`, linha 207; API depois de `onUpdateProgress`, linha 1351)

**Interfaces:**
- Consumes: canais da Task 3B.
- Produces: `window.launcherAPI.updatePackaged*` + `onUpdatePackagedChanged` tipados para a Task 4.

- [ ] **Step 1: Adicionar ao preload (sem tocar nos `update*` existentes)**

Em `app/electron/preload.js`, logo depois do bloco `onUpdateProgress` (linha 269), adicione:

```js
  // Canal empacotado (AppImage/NSIS) — não confundir com o updater git acima.
  updatePackagedState: () => ipcRenderer.invoke("update:packaged:state"),
  updatePackagedCheck: (data) => ipcRenderer.invoke("update:packaged:check", data),
  updatePackagedDownload: () => ipcRenderer.invoke("update:packaged:download"),
  updatePackagedInstall: () => ipcRenderer.invoke("update:packaged:install"),
  updatePackagedJaAvisado: (versao) => ipcRenderer.invoke("update:packaged:jaAvisado", { versao }),
  onUpdatePackagedChanged: (cb) => {
    const h = (_e, data) => cb(data)
    ipcRenderer.on("update:packaged:changed", h)
    return () => ipcRenderer.removeListener("update:packaged:changed", h)
  },
```

- [ ] **Step 2: Tipos do estado/checagem em `global.d.ts`**

Em `app/src/global.d.ts`, logo depois de `export type UpdateEtapa = ...` (linha 207), adicione:

```ts
export type UpdatePackagedCanal = "fonte" | "appimage" | "nsis" | "portable" | "zip" | "sem_suporte"
export type UpdatePackagedFase = "ocioso" | "disponivel" | "baixando" | "pronto" | "erro" | "sem_suporte"

/** Snapshot do updater do app empacotado (electron-updater). */
export interface UpdatePackagedState {
  canal: UpdatePackagedCanal
  suportado: boolean
  versaoAtual: string
  versaoNova: string | null
  /** Bytes do arquivo oferecido (updateInfo.files[].size), quando o yml traz. */
  tamanho: number | null
  fase: UpdatePackagedFase
  /** 0–100. */
  progresso: number
  erro: string | null
  /** true = erro da checagem automática; o diálogo não deve abrir (M3). */
  erroDeFundo: boolean
  jaAvisado: boolean
  jogoRodando: boolean
}

/** Retorno de `updatePackagedCheck`; `motivo` é opcional por contrato. */
export interface UpdatePackagedCheck {
  ok: boolean
  disponivel: boolean
  versao?: string | null
  tamanho?: number | null
  motivo?: "canal_nao_suportado" | "jogo_rodando" | "erro" | "fonte"
  erro?: string
}
```

- [ ] **Step 3: Tipar a API no `launcherAPI`**

Em `app/src/global.d.ts`, logo depois de `onUpdateProgress` (linha 1351), adicione:

```ts
      /** Estado do updater do app empacotado (AppImage/NSIS). */
      updatePackagedState: () => Promise<UpdatePackagedState>
      /** Checa agora; `manual` decide se o erro pode virar diálogo. */
      updatePackagedCheck: (data?: { manual?: boolean }) => Promise<UpdatePackagedCheck>
      /** Baixa o update oferecido (só depois do "Baixar" do usuário). */
      updatePackagedDownload: () => Promise<{ ok: boolean; erro?: string }>
      /** Reinicia e instala o update já baixado. */
      updatePackagedInstall: () => Promise<{ ok: boolean; erro?: string }>
      /** Marca "já avisei nesta versão" (persiste no config). */
      updatePackagedJaAvisado: (versao: string) => Promise<{ ok: boolean }>
      /** Estado do updater empacotado mudou (push do main). */
      onUpdatePackagedChanged: (cb: (estado: UpdatePackagedState) => void) => () => void
```

- [ ] **Step 4: Verificar**

Run: `cd app && npx tsc --noEmit && npm test 2>&1 | tail -5`

Expected: tsc limpo; suíte com a falha pré-existente.

- [ ] **Step 5: Commit**

```bash
git add app/electron/preload.js app/src/global.d.ts
git commit -m "feat(updater): preload e tipos do canal empacotado"
```

---

### Task 3D: Agendamento (30s/6h), gating e o push de boot

**Files:**
- Modify: `app/electron/main.js` (perto de `procurarAtualizacao`, linha 2510; e no `did-finish-load`, linha 2910)

**Interfaces:**
- Consumes: `atualizadorEmpacotado` (Task 3A) e `checar` (Task 2).
- Produces: ciclo automático de 30s + 6h gateado por `check_updates_on_start` e por `jogoRodando`; push inicial que cobre o aviso portable/zip; ramificação explícita do canal git.

- [ ] **Step 1: Ramificar o canal git por `app.isPackaged` (spec:42)**

Em `main.js:2510`, no começo de `procurarAtualizacao`, adicione a primeira linha do `try`:

```js
async function procurarAtualizacao(win) {
  try {
    // O canal git é de quem roda da fonte; no pacote quem cuida é o updater
    // empacotado. Ramo explícito — não depende do .git faltar por acaso.
    if (app.isPackaged) return
    if (readConfig().check_updates_on_start === false) return
    if (!(await updater.estado()).podeAtualizar) return
```

- [ ] **Step 2: O ciclo automático (instância já existe; só o ciclo é gateado)**

Em `main.js`, logo depois da função `procurarAtualizacao` (linha ~2518), adicione:

```js
// Canal empacotado: 30s depois da janela carregar e a cada 6h. Jogo rodando
// adia (reavalia no próximo ciclo) e o toggle check_updates_on_start desliga o
// ciclo automático — a instância e a checagem manual continuam funcionando.
const INTERVALO_EMPACOTADO_MS = 6 * 60 * 60 * 1000
function agendarCicloEmpacotado() {
  if (cicloEmpacotado) {
    clearTimeout(cicloEmpacotado)
    cicloEmpacotado = null
  }
  if (!atualizadorEmpacotado || !atualizadorEmpacotado.suportado()) return
  const rodar = async () => {
    try {
      if (readConfig().check_updates_on_start !== false) await atualizadorEmpacotado.checar()
    } catch {}
    cicloEmpacotado = setTimeout(rodar, INTERVALO_EMPACOTADO_MS)
  }
  cicloEmpacotado = setTimeout(rodar, 30_000)
}
```

- [ ] **Step 3: Push de boot + início do ciclo no `did-finish-load`**

Em `main.js:2910`, o `did-finish-load` termina com `procurarAtualizacao(win)`. Deixe assim:

```js
    // Atualização do Arcadia: verifica DEPOIS da janela carregar e sem
    // esperar — checar antes atrasaria a abertura por causa de uma ida à
    // rede que pode nem ter resposta.
    procurarAtualizacao(win)

    // Canal empacotado: o push inicial cobre o aviso portable/zip (fase
    // sem_suporte já no boot) e o ciclo automático começa aqui.
    if (atualizadorEmpacotado && atualizadorEmpacotado.canal() !== "fonte") {
      if (win && !win.isDestroyed()) {
        win.webContents.send("update:packaged:changed", atualizadorEmpacotado.estado())
      }
      agendarCicloEmpacotado()
    }
  })
```

- [ ] **Step 4: Verificar**

Run: `cd app && npx tsc --noEmit && npm test 2>&1 | tail -5 && npm run build 2>&1 | tail -1`

Expected: tsc limpo, suíte com a falha pré-existente, build ok.

- [ ] **Step 5: Commit**

```bash
git add app/electron/main.js
git commit -m "feat(updater): ciclo 30s/6h gateado pelo toggle + push de boot (portable/zip)"
```

---

### Task 4A: As chaves i18n nos 3 dicionários

**Files:**
- Modify: `app/src/i18n/pt-BR.json`, `app/src/i18n/en-US.json`, `app/src/i18n/es-ES.json`

**Interfaces:**
- Consumes: nada.
- Produces: 12 chaves `update.packaged.*` idênticas nos 3 dicionários (o teste `i18n-sem-duplicatas.test.js:35` exige conjuntos iguais).

- [ ] **Step 1: Adicionar as chaves**

Nos 3 arquivos, logo depois de `"update.etapa.pull"` (linha 1206 nos três), adicione os mesmos nomes:

`pt-BR.json`:

```json
  "update.packaged.titulo": "Atualização do Arcadia",
  "update.packaged.disponivel": "Arcadia {versao} disponível",
  "update.packaged.disponivel_tamanho": "Arcadia {versao} disponível — {tamanho}",
  "update.packaged.baixar": "Baixar",
  "update.packaged.depois": "Depois",
  "update.packaged.baixando": "Baixando… {pct}%",
  "update.packaged.pronto": "Pronto pra instalar",
  "update.packaged.reiniciar": "Reiniciar agora",
  "update.packaged.erro": "Não foi possível baixar. Tente de novo.",
  "update.packaged.sem_suporte": "Esta versão não se atualiza sozinha. Baixe a nova pela página de releases.",
  "update.packaged.abrir_release": "Abrir página",
  "update.packaged.em_dia": "Você já está na versão mais recente.",
```

`en-US.json`:

```json
  "update.packaged.titulo": "Arcadia update",
  "update.packaged.disponivel": "Arcadia {versao} is available",
  "update.packaged.disponivel_tamanho": "Arcadia {versao} is available — {tamanho}",
  "update.packaged.baixar": "Download",
  "update.packaged.depois": "Later",
  "update.packaged.baixando": "Downloading… {pct}%",
  "update.packaged.pronto": "Ready to install",
  "update.packaged.reiniciar": "Restart now",
  "update.packaged.erro": "Could not download. Try again.",
  "update.packaged.sem_suporte": "This build can't update itself. Download the new version from the releases page.",
  "update.packaged.abrir_release": "Open page",
  "update.packaged.em_dia": "You're on the latest version.",
```

`es-ES.json`:

```json
  "update.packaged.titulo": "Actualización de Arcadia",
  "update.packaged.disponivel": "Arcadia {versao} está disponible",
  "update.packaged.disponivel_tamanho": "Arcadia {versao} está disponible — {tamanho}",
  "update.packaged.baixar": "Descargar",
  "update.packaged.depois": "Después",
  "update.packaged.baixando": "Descargando… {pct}%",
  "update.packaged.pronto": "Listo para instalar",
  "update.packaged.reiniciar": "Reiniciar ahora",
  "update.packaged.erro": "No se pudo descargar. Inténtalo de nuevo.",
  "update.packaged.sem_suporte": "Esta versión no se actualiza sola. Descarga la nueva desde la página de releases.",
  "update.packaged.abrir_release": "Abrir página",
  "update.packaged.em_dia": "Ya estás en la última versión.",
```

- [ ] **Step 2: Verificar**

Run: `cd app && node --test test/i18n-sem-duplicatas.test.js && npx tsc --noEmit`

Expected: PASS nos 4 testes do i18n; tsc limpo.

- [ ] **Step 3: Commit**

```bash
git add app/src/i18n/pt-BR.json app/src/i18n/en-US.json app/src/i18n/es-ES.json
git commit -m "feat(updater): i18n do canal empacotado (disponivel/baixando/pronto/sem suporte)"
```

---

### Task 4B: O diálogo do canal empacotado + o hook que o abre (B2)

**Files:**
- Modify: `app/src/components/UpdateDialog.tsx`

**Interfaces:**
- Consumes: `window.launcherAPI.updatePackaged*`/`onUpdatePackagedChanged` (Task 3C) e as chaves da Task 4A.
- Produces:
  - `UpdatePackagedDialog({ estado, console?, onBaixar, onInstalar, onDepois })`
  - `useAtualizacaoEmpacotada()` → `{ estado, dispensar, baixar, instalar }` — abre por `disponivel` (se `!jaAvisado`), `pronto`, `sem_suporte` e erro de ação; **silencia** `ocioso`, `baixando` e erro de fundo.

- [ ] **Step 1: Importar o tipo e o formatador de bytes**

Em `app/src/components/UpdateDialog.tsx`, troque as linhas 4-6:

```tsx
import type { UpdateEtapa, UpdateInfo, UpdatePackagedState } from "../global"
import { useI18n } from "../i18n/I18nContext"
import { useGamepadNav } from "./ps5-launcher/useGamepadNav"
import { fmtBytes } from "./tamanho"
```

- [ ] **Step 2: Adicionar o diálogo e o hook no fim do arquivo**

No fim de `UpdateDialog.tsx` (depois de `useAtualizacao`), adicione:

```tsx
const RELEASES_URL = "https://github.com/imperat-on/arcadia/releases"

interface UpdatePackagedDialogProps {
  estado: UpdatePackagedState
  /** Big Picture: liga a navegação por controle e escurece mais o fundo. */
  console?: boolean
  onBaixar: () => void
  onInstalar: () => void
  onDepois: () => void
}

// Diálogo do canal empacotado. Mesmo visual do UpdateDialog do git, mas com as
// fases do electron-updater (disponivel/baixando/pronto/erro/sem_suporte).
export function UpdatePackagedDialog({
  estado,
  console: modoConsole = false,
  onBaixar,
  onInstalar,
  onDepois,
}: UpdatePackagedDialogProps) {
  const { t } = useI18n()
  const ref = useRef<HTMLDivElement>(null)
  const baixando = estado.fase === "baixando"
  useGamepadNav(ref, modoConsole && !baixando, onDepois)

  const versao = estado.versaoNova || estado.versaoAtual
  const subtitulo =
    estado.fase === "sem_suporte"
      ? t("update.packaged.sem_suporte")
      : estado.fase === "disponivel"
        ? estado.tamanho
          ? t("update.packaged.disponivel_tamanho", { versao, tamanho: fmtBytes(estado.tamanho) })
          : t("update.packaged.disponivel", { versao })
        : estado.fase === "baixando"
          ? t("update.packaged.baixando", { pct: estado.progresso })
          : estado.fase === "pronto"
            ? t("update.packaged.pronto")
            : t("update.packaged.erro")

  return (
    <div
      className="fixed inset-0 z-[80] flex items-center justify-center backdrop-blur-sm"
      style={{ background: modoConsole ? "rgba(0,0,0,0.85)" : "rgba(0,0,0,0.6)" }}
    >
      <div
        ref={ref}
        className="gp-scope w-[460px] max-w-[92vw] rounded-2xl border border-white/[0.08] p-6 shadow-2xl"
        style={{ background: modoConsole ? "rgba(10,12,20,0.98)" : "var(--surface-1)" }}
        role="dialog"
        aria-label={t("update.packaged.titulo")}
      >
        <h3 className="mb-1 text-lg font-semibold text-white">{t("update.packaged.titulo")}</h3>
        <p className="mb-4 text-[13px] text-white/60">{subtitulo}</p>

        {baixando && (
          <div className="mb-4 h-2 overflow-hidden rounded-full bg-white/[0.08]">
            <div
              className="h-full rounded-full transition-[width] duration-300"
              style={{ width: `${estado.progresso}%`, background: "var(--accent)" }}
            />
          </div>
        )}

        {estado.fase === "erro" && estado.erro && (
          <p className="mb-3 text-[12px] text-white/40">{estado.erro}</p>
        )}

        {!baixando && (
          <div className="flex justify-end gap-2.5">
            <button
              onClick={onDepois}
              className="rounded-lg border border-white/15 px-5 py-2.5 text-[12px] font-semibold text-white/70 transition-colors hover:bg-white/[0.06] hover:text-white"
            >
              {t("update.packaged.depois")}
            </button>
            {estado.fase === "sem_suporte" ? (
              <button
                onClick={() => window.launcherAPI?.openExternal(RELEASES_URL)}
                className="rounded-lg px-5 py-2.5 text-[12px] font-bold text-black transition-transform hover:scale-[1.03]"
                style={{ background: "var(--accent)" }}
              >
                {t("update.packaged.abrir_release")}
              </button>
            ) : estado.fase === "pronto" ? (
              <button
                onClick={onInstalar}
                className="rounded-lg px-5 py-2.5 text-[12px] font-bold text-black transition-transform hover:scale-[1.03]"
                style={{ background: "var(--accent)" }}
              >
                {t("update.packaged.reiniciar")}
              </button>
            ) : (
              <button
                onClick={onBaixar}
                className="rounded-lg px-5 py-2.5 text-[12px] font-bold text-black transition-transform hover:scale-[1.03]"
                style={{ background: "var(--accent)" }}
              >
                {t("update.packaged.baixar")}
              </button>
            )}
          </div>
        )}
      </div>
    </div>
  )
}

/**
 * Assina o canal empacotado e decide quando o diálogo abre:
 * - abre: `disponivel` (se `!jaAvisado`), `pronto` (reaviso do boot, D6),
 *   `sem_suporte` (portable/zip/AppImage extraído, no boot) e `erro` de ação
 *   do usuário (`erroDeFundo === false`);
 * - silencia: `ocioso`, `baixando` (só continua um diálogo já aberto) e erro
 *   de fundo (sem rede no ciclo de 6h não pode virar aviso).
 * "Depois" grava a versão no config (D7) e não reabre nesta sessão.
 */
export function useAtualizacaoEmpacotada() {
  const [estado, setEstado] = useState<UpdatePackagedState | null>(null)
  const dispensados = useRef<Set<string>>(new Set())

  useEffect(() => {
    let vivo = true
    const chave = (e: UpdatePackagedState) => `${e.fase}:${e.versaoNova ?? e.versaoAtual}`
    const decidir = (e: UpdatePackagedState): UpdatePackagedState | null => {
      if (e.canal === "fonte") return null
      if (e.fase === "sem_suporte") return dispensados.current.has(chave(e)) ? null : e
      if (e.fase === "disponivel") return e.jaAvisado || dispensados.current.has(chave(e)) ? null : e
      if (e.fase === "pronto") return dispensados.current.has(chave(e)) ? null : e
      if (e.fase === "erro") return e.erroDeFundo ? null : e
      return null
    }
    window.launcherAPI
      ?.updatePackagedState?.()
      .then((e) => {
        if (!vivo || !e) return
        const aberto = decidir(e)
        if (aberto) setEstado(aberto)
      })
    const off = window.launcherAPI?.onUpdatePackagedChanged?.((e) => {
      if (!vivo || !e) return
      setEstado((atual) => {
        if (e.fase === "baixando") return atual ? { ...atual, ...e } : atual
        if (e.fase === "ocioso") return null
        return decidir(e)
      })
    })
    return () => {
      vivo = false
      off?.()
    }
  }, [])

  const dispensar = () => {
    setEstado((atual) => {
      if (atual) {
        dispensados.current.add(`${atual.fase}:${atual.versaoNova ?? atual.versaoAtual}`)
        if (atual.fase === "disponivel" && atual.versaoNova) {
          void window.launcherAPI?.updatePackagedJaAvisado?.(atual.versaoNova)
        }
      }
      return null
    })
  }

  const baixar = async () => {
    setEstado((atual) => (atual ? { ...atual, fase: "baixando", progresso: 0, erro: null } : atual))
    const r = await window.launcherAPI?.updatePackagedDownload?.()
    if (!r?.ok) {
      setEstado((atual) =>
        atual ? { ...atual, fase: "erro", erro: r?.erro || "erro", erroDeFundo: false } : atual,
      )
    }
  }

  const instalar = async () => {
    const r = await window.launcherAPI?.updatePackagedInstall?.()
    if (!r?.ok) {
      setEstado((atual) =>
        atual ? { ...atual, fase: "erro", erro: r?.erro || "erro", erroDeFundo: false } : atual,
      )
    }
  }

  return { estado, dispensar, baixar, instalar }
}
```

- [ ] **Step 3: Verificar**

Run: `cd app && npx tsc --noEmit && npm test 2>&1 | tail -5 && npm run build 2>&1 | tail -1`

Expected: tsc limpo; i18n verde; suíte com a falha pré-existente; build ok.

- [ ] **Step 4: Commit**

```bash
git add app/src/components/UpdateDialog.tsx
git commit -m "feat(updater): dialogo do canal empacotado + hook que o abre (B2)"
```

---

### Task 4C: Ligar nos dois launchers e no Settings (B1/B2/M4)

**Files:**
- Modify: `app/src/components/desktop/DesktopLauncher.tsx` (import na linha 21; hook na 228; render na 503)
- Modify: `app/src/components/ps5-launcher/PS5Launcher.tsx` (import na linha 30; hook na 173; render na 1772)
- Modify: `app/src/components/desktop/GeneralSection.tsx` (função `ProcurarAtualizacao`, linhas 189-229)

**Interfaces:**
- Consumes: `UpdatePackagedDialog`/`useAtualizacaoEmpacotada` (Task 4B).
- Produces: diálogo automático nos dois modos; botão manual de Configurações ramificando por canal (sem a mensagem "não é um clone do Git").

- [ ] **Step 1: DesktopLauncher**

Linha 21, troque o import:

```tsx
import { UpdateDialog, UpdatePackagedDialog, useAtualizacao, useAtualizacaoEmpacotada } from "../UpdateDialog"
```

Linha 228, logo depois de `const atualizacao = useAtualizacao()`, adicione:

```tsx
  const atualizacaoEmpacotada = useAtualizacaoEmpacotada()
```

Linha 503, logo depois do bloco `{atualizacao.info && (<UpdateDialog ... />)}`, adicione:

```tsx
      {atualizacaoEmpacotada.estado && (
        <UpdatePackagedDialog
          estado={atualizacaoEmpacotada.estado}
          onBaixar={atualizacaoEmpacotada.baixar}
          onInstalar={atualizacaoEmpacotada.instalar}
          onDepois={atualizacaoEmpacotada.dispensar}
        />
      )}
```

- [ ] **Step 2: PS5Launcher**

Linha 30, troque o import:

```tsx
import { UpdateDialog, UpdatePackagedDialog, useAtualizacao, useAtualizacaoEmpacotada } from "../UpdateDialog"
```

Linha 173, logo depois de `const atualizacao = useAtualizacao()`, adicione:

```tsx
  const atualizacaoEmpacotada = useAtualizacaoEmpacotada()
```

Linha 1772, logo depois do bloco `{atualizacao.info && (<UpdateDialog ... console ... />)}`, adicione:

```tsx
      {/* Canal empacotado: A baixa/reinicia, B adia. */}
      {atualizacaoEmpacotada.estado && (
        <UpdatePackagedDialog
          estado={atualizacaoEmpacotada.estado}
          console
          onBaixar={atualizacaoEmpacotada.baixar}
          onInstalar={atualizacaoEmpacotada.instalar}
          onDepois={atualizacaoEmpacotada.dispensar}
        />
      )}
```

- [ ] **Step 3: GeneralSection — o botão "Procurar atualizações" ramifica por canal (M4)**

Substitua a função `ProcurarAtualizacao` inteira (linhas 189-229) por:

```tsx
// Verificação manual, para quem desligou a automática — e o único lugar onde
// o motivo de o clone estar bloqueado aparece por escrito. Sem isto, quem tem
// alteração local nunca saberia por que o aviso não vem.
function ProcurarAtualizacao() {
  const { t } = useI18n()
  const [msg, setMsg] = useState("")
  const [busy, setBusy] = useState(false)
  const [info, setInfo] = useState<UpdateInfo | null>(null)

  const procurar = async () => {
    setBusy(true)
    setMsg("")
    // M4: o renderer não tem `isPackaged`; o canal do estado novo decide.
    // No app empacotado não existe `.git` — a mensagem "não é um clone do
    // Git" não se aplica e some.
    const canal = (await window.launcherAPI?.updatePackagedState())?.canal
    if (canal && canal !== "fonte") {
      const r = await window.launcherAPI?.updatePackagedCheck({ manual: true })
      setBusy(false)
      if (r?.motivo === "canal_nao_suportado") return setMsg(t("update.packaged.sem_suporte"))
      if (!r?.ok) return setMsg(r?.erro || t("update.erro_generico"))
      if (!r.disponivel) return setMsg(t("update.packaged.em_dia"))
      // A checagem manual zera o "já avisei" no módulo; o diálogo do launcher
      // abre pelo push `update:packaged:changed` — nada a fazer aqui.
      return
    }
    const st = await window.launcherAPI?.updateState()
    if (st && !st.podeAtualizar) {
      setBusy(false)
      setMsg(t(`update.bloqueado.${st.motivo}`, { detalhe: st.detalhe || "" }))
      return
    }
    const r = await window.launcherAPI?.updateCheck()
    setBusy(false)
    if (!r?.ok) return setMsg(r?.error || t("update.erro_generico"))
    if (!r.atrasado) return setMsg(t("update.em_dia", { sha: r.local || "" }))
    setInfo(r)
  }

  return (
    <>
      <Row
        label={t("update.procurar.label")}
        desc={msg || t("update.procurar.desc")}
        control={
          <button
            onClick={procurar}
            disabled={busy}
            className="rounded-lg border border-white/10 bg-[color:var(--surface-2)] px-3 py-1.5 text-[12px] text-white/70 outline-none transition-colors hover:border-white/25 hover:text-white disabled:opacity-60"
          >
            {busy ? t("update.procurando") : t("update.procurar")}
          </button>
        }
      />
      {info && <UpdateDialog info={info} onDepois={() => setInfo(null)} />}
    </>
  )
}
```

- [ ] **Step 4: Verificar**

Run: `cd app && npx tsc --noEmit && npm test 2>&1 | tail -5 && npm run build 2>&1 | tail -1`

Expected: tsc limpo; suíte com a falha pré-existente; build ok.

- [ ] **Step 5: Commit**

```bash
git add app/src/components/desktop/DesktopLauncher.tsx app/src/components/ps5-launcher/PS5Launcher.tsx app/src/components/desktop/GeneralSection.tsx
git commit -m "feat(updater): liga o dialogo empacotado nos dois launchers e no Settings (B2/M4)"
```

---

### Task 5: Fluxo de release (os yml no lugar certo) + bump + limitações

**Files:**
- Create: `docs/release.md`
- Modify: `README.md` (nova seção "Releasing" antes de `## Uninstall`, linha 81)
- Modify: `app/package.json` (`"version": "1.4.1"` → `"1.4.2"`)

**Interfaces:**
- Consumes: os artefatos da Task 1.
- Produces: checklist do release que garante o par certo de yml; limitações aceitas documentadas (Windows sem assinatura, UAC do NSIS, portable/zip, AppImage sem escrita); versão 1.4.2 pronta para o teste de ponta a ponta.

- [ ] **Step 1: Criar `docs/release.md` com o checklist completo**

Crie `docs/release.md` com:

```markdown
# Release do Arcadia (canal empacotado)

O updater do app empacotado (AppImage/NSIS) lê o `latest*.yml` anexado ao
release. Anexar o yml errado (ou esquecer) faz o app baixar a versão errada —
siga a ordem.

## Checklist

1. **Versão** — confira `"version"` em `app/package.json` (ex.: `1.4.2`).
2. **Limpe os yml antigos** — `rm -f app/release/latest*.yml`. O `release/`
   acumula builds anteriores (1.2.3 a 1.4.1) e reaproveitar um yml velho faz o
   updater oferecer uma versão que não é a publicada.
3. **Builds da MESMA versão**:

   ```bash
   cd app
   npm run dist:appimage   # gera release/latest-linux.yml
   npm run dist:nsis       # gera release/latest.yml
   ```

4. **Confira os yml gerados**:

   ```bash
   grep -H '^version:' release/latest.yml release/latest-linux.yml
   ```

   As duas linhas têm que mostrar a versão do passo 1.
5. **Publique os 4 assets + os 2 yml** (não-draft e não-prerelease: o
   electron-updater ignora draft/prerelease):

   ```bash
   gh release create v1.4.2 --title "Arcadia 1.4.2" \
     release/Arcadia-Setup-1.4.2-x64.exe \
     release/Arcadia-1.4.2-x64.exe \
     release/Arcadia-1.4.2-x64.zip \
     release/Arcadia-1.4.2-x86_64.AppImage \
     release/latest.yml release/latest-linux.yml
   ```

6. **Valide o release publicado**:

   ```bash
   gh release view v1.4.2 --json assets --jq '.assets[].name' | sort
   ```

   Esperado: `Arcadia-1.4.2-x64.exe`, `Arcadia-1.4.2-x64.zip`,
   `Arcadia-1.4.2-x86_64.AppImage`, `Arcadia-Setup-1.4.2-x64.exe`,
   `latest-linux.yml`, `latest.yml`.

   > O `v1.4.1` foi publicado **antes** deste recurso e não tem yml nenhum —
   > validar por ele não serve. A validação é sempre do release que acabou de
   > ser publicado.

## Limitações aceitas (documentadas)

- **Windows sem assinatura de código**: o updater funciona, mas
  SmartScreen/Defender avisam tanto no setup quanto no update. Exige
  certificado pago — fora de escopo por agora.
- **NSIS por-máquina** (`oneClick: false`, `allowToChangeInstallationDirectory:
  true`): o update dispara UAC. Se o usuário negar, a instalação não acontece e
  o aviso "Pronto — Reiniciar agora" volta no próximo boot.
- **Portable e zip** não têm auto-update: o app mostra "Esta versão não se
  atualiza sozinha" com link para a página de releases.
- **AppImage em pasta sem escrita** (`/opt`, montagem read-only): o
  `quitAndInstall` falha e o app mostra o aviso com link.
- **Download parcial não retoma byte a byte** (electron-updater 6 descarta o
  temporário): o arquivo **completo** em cache é reaproveitado no próximo boot;
  no NSIS o download diferencial reduz o volume quando há blockmap.
```

- [ ] **Step 2: Adicionar a seção "Releasing" no README**

Em `README.md`, antes de `## Uninstall` (linha 81), adicione:

```markdown
## Releasing

Releases are built and published manually. The packaged app auto-updates from
the `latest*.yml` files attached to each release, so the checklist matters:
see [docs/release.md](docs/release.md) before publishing.
```

- [ ] **Step 3: Bump de versão para 1.4.2**

Em `app/package.json`, troque `"version": "1.4.1"` por:

```json
  "version": "1.4.2",
```

- [ ] **Step 4: Verificar**

Run: `cd app && node -e "console.log(require('./package.json').version)" && grep -n '^version:' release/latest.yml 2>/dev/null; true`

Expected: `1.4.2` (os yml locais, se existirem, são de builds anteriores — o `docs/release.md` manda apagá-los antes do release).

- [ ] **Step 5: Commit**

```bash
git add docs/release.md README.md app/package.json
git commit -m "docs: checklist de release com os yml do updater + bump 1.4.2 + limitacoes (assinatura/UAC)"
```

---

### Task 6: Validação de ponta a ponta

**Files:**
- Modify: nenhum (validação)

- [ ] **Step 1: Suíte completa e build**

Run: `cd app && npx tsc --noEmit && npm test 2>&1 | tail -5 && npm run build 2>&1 | tail -1`

Expected: 1 falha pré-existente (`steam-path`), resto verde; os 17 testes de `updater-packaged.test.js` passando.

- [ ] **Step 2: O canal da fonte não regrediu**

Run: `cd ~/Documents/projects/arcadia && ./arcadia.sh` (alguns segundos) e confira no log que o updater git continua sendo acionado e que o módulo empacotado não faz nada (`isPackaged === false` → `canal() === "fonte"`, nenhum push `update:packaged:changed`).

- [ ] **Step 3: Teste manual do canal empacotado (documentado no PR)**

Siga `docs/release.md` para publicar 1.4.2 (builds + yml + assets). Depois:
1. rode o AppImage 1.4.1 e espere o diálogo (30s);
2. escolha "Baixar" e confira o progresso;
3. escolha "Depois" e feche; reabra: o diálogo "Pronto — Reiniciar agora" volta no boot (D6);
4. escolha "Reiniciar agora" e valide a versão nova (1.4.2);
5. rode o portable/zip: o diálogo "não se atualiza sozinha" com link aparece no boot (sem suporte);
6. no Windows, se disponível: instale o NSIS e repita (1→4), registrando o comportamento do SmartScreen/Defender e do UAC (limitações do `docs/release.md`).

- [ ] **Step 4: Reportar**

No relatório: os números da suíte, o resultado do teste manual, os itens não validados (Windows/UAC, se não houver máquina) e o que ficou fora de escopo (assinatura de código, delta, canais beta).
