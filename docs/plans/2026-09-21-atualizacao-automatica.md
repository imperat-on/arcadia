# Atualização automática (canal empacotado) — plano de implementação

> **For agentic workers:** REQUIRED SUB-SKILL: use `subagent-driven-development` (recomendado) ou `executing-plans` para implementar task por task. Passos em checkbox (`- [ ]`).

**Goal:** o app empacotado (AppImage/NSIS) avisa que existe versão nova, baixa só se o usuário aceitar, e aplica no restart — sem tocar no updater git de quem roda da fonte.

**Architecture:** um módulo novo (`electron/updater-packaged.js`) encapsula o `electron-updater` com dependências injetáveis (para testar sem Electron), o `main.js` o instancia só quando `app.isPackaged` e expõe IPC `update:packaged:*`; a UI reusa o `UpdateDialog.tsx` existente com estados novos. O build passa a gerar `app-update.yml` + `latest*.yml`, e o release passa a anexá-los.

**Tech Stack:** Electron 33, electron-updater, electron-builder (NSIS/AppImage), React + TypeScript (Vite), `node --test` (sem Electron na suíte).

**Spec:** `docs/specs/2026-09-20-atualizacao-automatica.md` (commit `24ce0d0`) — leia junto; este plano argumenta a partir dele.

## Global Constraints

- **Não tocar** em `app/electron/updater.js` (canal git), nos IPC `update:state`/`update:check`/`update:apply`, nem em `preload.js:256-269` — o canal da fonte fica exatamente como está.
- Nomes novos: módulo `app/electron/updater-packaged.js`; canais IPC `update:packaged:state|check|download|install`.
- `electron-updater` vai em **`dependencies`** (nunca `devDependencies`).
- `autoDownload: false` e `autoInstallOnAppQuit: false` são invariantes do desenho.
- Todo texto de UI novo entra nos **3** dicionários (`pt-BR`, `en-US`, `es-ES`) — o teste `i18n-sem-duplicatas.test.js:35` exige conjuntos idênticos.
- A suíte é `node --test` puro: nada de `import { app } from "electron"` em código de teste.
- Textos curtos e diretos (padrão do dono).
- Verificação por task: `npx tsc --noEmit`, `npm test` (esperado: 1 falha pré-existente, `steam-path`), `npm run build`.

---

### Task 1: Build e publicação (o updater precisa do cardápio)

**Files:**
- Modify: `app/package.json` (bloco `build`, `dependencies`, `scripts.dist:*`)

**Interfaces:**
- Produces: `build.publish` (provider github), `repository`, `electron-updater` disponível para `require`; os builds passam a escrever `resources/app-update.yml` e `release/latest*.yml`.

- [ ] **Step 1: Adicionar `repository` e `publish` ao package.json**

Em `app/package.json`, logo após `"version"`, adicione:

```json
  "repository": { "type": "git", "url": "https://github.com/imperat-on/arcadia.git" },
```

Dentro de `"build"`, acrescente:

```json
    "publish": { "provider": "github", "owner": "imperat-on", "repo": "arcadia" },
```

- [ ] **Step 2: Instalar o electron-updater como dependência de runtime**

Run: `cd app && npm install electron-updater --save`
Expected: entra em `"dependencies"` (confira: `python3 -c "import json;print('electron-updater' in json.load(open('package.json'))['dependencies'])"` → `True`).

- [ ] **Step 3: Blindar os scripts contra publish implícito**

Troque os 4 scripts:

```json
    "dist:appimage": "vite build && electron-builder --linux AppImage --x64 --publish never",
    "dist:nsis": "vite build && electron-builder --win nsis --x64 --publish never",
    "dist:portable": "vite build && electron-builder --win portable --x64 --publish never",
    "dist:zip": "vite build && electron-builder --win zip --x64 --publish never",
```

- [ ] **Step 4: Provar que o build gera o cardápio**

Run: `cd app && npm run dist:nsis 2>&1 | tail -5 && ls -la release/latest.yml release/win-unpacked/resources/app-update.yml`
Expected: `latest.yml` na raiz de `release/` e `app-update.yml` dentro de `resources/` do pacote.

- [ ] **Step 5: Provar o yml do Linux**

Run: `cd app && npm run dist:appimage 2>&1 | tail -5 && ls -la release/latest-linux.yml`
Expected: `latest-linux.yml` presente. (Nota: `portable` e `zip` NÃO geram yml — comportamento esperado.)

- [ ] **Step 6: Commit**

```bash
git add app/package.json app/package-lock.json
git commit -m "build: publish github + electron-updater em dependencies + --publish never"
```

---

### Task 2: O módulo do updater empacotado (injetável e testável)

**Files:**
- Create: `app/electron/updater-packaged.js`
- Test: `app/test/updater-packaged.test.js`

**Interfaces:**
- Consumes: nada das outras tasks.
- Produces (contrato que a Task 3 e a Task 4 usam):
  - `createPackagedUpdater({ app, autoUpdater, env, isJogoRodando, jaAvisado, salvarJaAvisado })` → `{ estado(), checar(), baixar(), instalar(), canal() }`
  - `estado()` → `{ canal: "nsis"|"appimage"|"portable"|"zip"|"fonte", versaoAtual: string, versaoNova: string|null, fase: "ocioso"|"disponivel"|"baixando"|"pronto"|"erro", progresso: number, erro: string|null, jaAvisado: boolean }`
  - `checar()` → `Promise<{ disponivel: boolean, versao?: string }>` (NUNCA baixa sozinho)
  - `baixar()` → `Promise<{ ok: boolean, erro?: string }>` (só é chamado depois do "Baixar" do usuário)
  - `instalar()` → `{ ok: boolean, erro?: string }` (chama `quitAndInstall`)
  - `canal()` → `"nsis"|"appimage"|"portable"|"zip"|"fonte"`

- [ ] **Step 1: Escrever o teste que falha (detecção de canal)**

Crie `app/test/updater-packaged.test.js`:

```js
import test from "node:test"
import assert from "node:assert/strict"
import { createRequire } from "node:module"

const require = createRequire(import.meta.url)
const { createPackagedUpdater } = require("../electron/updater-packaged.js")

const fakeUpdater = () => {
  const listeners = {}
  return {
    autoDownload: true,
    autoInstallOnAppQuit: true,
    on: (ev, fn) => { (listeners[ev] ||= []).push(fn) },
    emitir: (ev, arg) => (listeners[ev] || []).forEach((fn) => fn(arg)),
    checkForUpdates: async () => ({ updateInfo: { version: "9.9.9" } }),
    downloadUpdate: async () => {},
    quitAndInstall: () => { fakeUpdater.instalou = true },
  }
}

const base = (over = {}) => ({
  app: { isPackaged: true, getVersion: () => "1.4.1" },
  autoUpdater: fakeUpdater(),
  env: {},
  temAppUpdateYml: true,
  ...over,
})

test("canal: fonte quando nao empacotado", () => {
  const u = createPackagedUpdater(base({ app: { isPackaged: false, getVersion: () => "1.4.1" } }))
  assert.equal(u.canal(), "fonte")
})

test("canal: appimage pela env APPIMAGE", () => {
  const u = createPackagedUpdater(base({ env: { APPIMAGE: "/tmp/Arcadia.AppImage" } }))
  assert.equal(u.canal(), "appimage")
})

test("canal: portable pela env PORTABLE_EXECUTABLE_FILE", () => {
  const u = createPackagedUpdater(base({ env: { PORTABLE_EXECUTABLE_FILE: "C:\\\\Arcadia.exe" } }))
  assert.equal(u.canal(), "portable")
})

test("canal: zip quando falta o app-update.yml", () => {
  const u = createPackagedUpdater(base({ temAppUpdateYml: false }))
  assert.equal(u.canal(), "zip")
})

test("checar NAO baixa sozinho (autoDownload desligado)", async () => {
  const up = fakeUpdater()
  const u = createPackagedUpdater(base({ autoUpdater: up }))
  const r = await u.checar()
  assert.equal(r.disponivel, true)
  assert.equal(r.versao, "9.9.9")
  assert.equal(up.autoDownload, false)
  assert.equal(up.autoInstallOnAppQuit, false)
  assert.equal(u.estado().fase, "disponivel")
})

test("baixar so roda quando chamado, e emite progresso", async () => {
  const up = fakeUpdater()
  let baixou = false
  up.downloadUpdate = async () => { baixou = true }
  const u = createPackagedUpdater(base({ autoUpdater: up }))
  await u.checar()
  assert.equal(baixou, false, "nao pode baixar antes do usuario aceitar")
  await u.baixar()
  assert.equal(baixou, true)
})

test("evento error vira estado de erro sem derrubar", () => {
  const up = fakeUpdater()
  const u = createPackagedUpdater(base({ autoUpdater: up }))
  up.emitir("error", new Error("sem rede"))
  assert.equal(u.estado().fase, "erro")
  assert.match(String(u.estado().erro), /sem rede/)
})

test("canal nao suportado nao checa e nao quebra", async () => {
  const u = createPackagedUpdater(base({ env: { PORTABLE_EXECUTABLE_FILE: "x" } }))
  const r = await u.checar()
  assert.equal(r.disponivel, false)
  assert.equal(r.motivo, "canal_nao_suportado")
})
```

- [ ] **Step 2: Rodar e ver falhar**

Run: `cd app && node --test test/updater-packaged.test.js`
Expected: FAIL — `Cannot find module '../electron/updater-packaged.js'`.

- [ ] **Step 3: Implementar o módulo**

Crie `app/electron/updater-packaged.js` com a mesma assinatura testada: `canal()` decide pela ordem `fonte → appimage → portable → zip(sem app-update.yml) → nsis`; `checar()` retorna cedo com `{ disponivel: false, motivo: "canal_nao_suportado" }` quando o canal não é suportado; configura `autoDownload=false` e `autoInstallOnAppQuit=false` no construtor; registra listeners de `update-available`, `download-progress`, `update-downloaded` e **`error`** (o handler de `error` só muda o estado — nunca re-lança); `baixar()` chama `downloadUpdate()`; `instalar()` chama `quitAndInstall()`; `estado()` devolve o snapshot do contrato acima; aceita `jaAvisado`/`salvarJaAvisado` opcionais para o estado "já avisei nesta versão".

- [ ] **Step 4: Rodar e ver passar**

Run: `cd app && node --test test/updater-packaged.test.js`
Expected: PASS (8 testes).

- [ ] **Step 5: Suíte e tipos**

Run: `cd app && npx tsc --noEmit && npm test 2>&1 | tail -5`
Expected: tsc limpo; suíte com a única falha pré-existente (`steam-path`).

- [ ] **Step 6: Commit**

```bash
git add app/electron/updater-packaged.js app/test/updater-packaged.test.js
git commit -m "feat(updater): modulo do canal empacotado (electron-updater, pergunta antes de baixar)"
```

---

### Task 3: Wiring no main (boot, IPC e adiamento com jogo rodando)

**Files:**
- Modify: `app/electron/main.js` (boot ~2510, IPC perto de 4118, `preload.js` e `global.d.ts`)

**Interfaces:**
- Consumes: `createPackagedUpdater` da Task 2 (o contrato exato acima).
- Produces: IPC `update:packaged:state|check|download|install` e o evento de push `update:packaged:changed` (o renderer assina); respeita `check_updates_on_start` e adia com jogo rodando.

- [ ] **Step 1: Instanciar no boot, só quando empacotado**

Em `main.js`, junto do `require("./updater")` existente (linha 53), carregue o módulo novo e, no boot (perto de 2510), crie a instância com `{ app, autoUpdater: require("electron-updater").autoUpdater, env: process.env, temAppUpdateYml: fs.existsSync(path.join(process.resourcesPath, "app-update.yml")), isJogoRodando }`. **Não** altere o caminho do updater git.

- [ ] **Step 2: Os IPC novos**

Perto dos IPC `update:*` (4118-4142), registre:

```js
ipcMain.handle("update:packaged:state", () => instancia?.estado() ?? { canal: "fonte" })
ipcMain.handle("update:packaged:check", async () => (instancia ? instancia.checar() : { disponivel: false, motivo: "fonte" }))
ipcMain.handle("update:packaged:download", async () => (instancia ? instancia.baixar() : { ok: false, erro: "fonte" }))
ipcMain.handle("update:packaged:install", () => (instancia ? instancia.instalar() : { ok: false, erro: "fonte" }))
```

E faça o módulo emitir mudanças de estado para a janela (`janela?.webContents.send("update:packaged:changed", estado)`).

- [ ] **Step 3: Expor no preload e tipar**

Em `preload.js`, adicione `updatePackagedState/Check/Download/Install` + `onUpdatePackagedChanged`; em `global.d.ts`, o tipo do retorno (mesmo shape de `estado()`). **Não** mexa nos `update*` existentes (256-269).

- [ ] **Step 4: Adiar com jogo rodando e respeitar o toggle**

Na checagem do boot (e no ciclo de 6h), pule quando `isJogoRodando()` for verdadeiro; só inicie o ciclo quando `check_updates_on_start` estiver ligado (a leitura do config já existe em `main.js:2512`).

- [ ] **Step 5: Verificar**

Run: `cd app && npx tsc --noEmit && npm test 2>&1 | tail -5 && npm run build 2>&1 | tail -1`
Expected: tsc limpo, suíte com a falha pré-existente, build ok.

- [ ] **Step 6: Commit**

```bash
git add app/electron/main.js app/electron/preload.js app/src/global.d.ts
git commit -m "feat(updater): wiring do canal empacotado no main (IPC novo, adia com jogo rodando)"
```

---

### Task 4: UI (o diálogo existente com os estados novos)

**Files:**
- Modify: `app/src/components/ps5-launcher/UpdateDialog.tsx`, `app/src/components/desktop/settings/GeneralSection.tsx`, `app/src/i18n/{pt-BR,en-US,es-ES}.json`

**Interfaces:**
- Consumes: os IPC da Task 3.
- Produces: diálogo com os estados `disponivel` (Baixar/Depois), `baixando` (progresso %), `pronto` (Reiniciar agora/Depois), `erro` (mensagem curta + Baixar de novo); funciona nos dois modos (gamepad preservado).

- [ ] **Step 1: As chaves i18n nos 3 dicionários**

Adicione (mesmos nomes nos 3 arquivos): `update.packaged.disponivel` ("Arcadia {versao} disponível"), `update.packaged.baixar` ("Baixar"), `update.packaged.depois` ("Depois"), `update.packaged.baixando` ("Baixando… {pct}%"), `update.packaged.pronto` ("Pronto pra instalar"), `update.packaged.reiniciar` ("Reiniciar agora"), `update.packaged.erro` ("Não foi possível baixar"), `update.packaged.sem_suporte` ("Baixe a versão nova"), `update.packaged.abrir_release` ("Abrir página").

- [ ] **Step 2: Estender o UpdateDialog com os estados novos**

No `UpdateDialog.tsx`, aceite as props do canal empacotado e renderize por fase, mantendo a navegação por controle (a lógica de foco já existe em `UpdateDialog.tsx:35`). Estados: `disponivel` (dois botões), `baixando` (barra + %), `pronto` (dois botões), `erro` (mensagem + Baixar), `sem_suporte` (aviso + Abrir página do release).

- [ ] **Step 3: Ligar no Settings (o toggle e o botão que já existem)**

Em `GeneralSection.tsx` (117-122 e 189-229), o toggle existente (`check_updates_on_start`) passa a valer também para o canal empacotado e o botão "Procurar atualizações" usa o IPC novo quando `isPackaged` (sem a mensagem "não é um clone do Git").

- [ ] **Step 4: Verificar**

Run: `cd app && npx tsc --noEmit && npm test 2>&1 | tail -5 && npm run build 2>&1 | tail -1`
Expected: tsc limpo; o teste de i18n continua verde (conjuntos idênticos nos 3); suíte com a falha pré-existente; build ok.

- [ ] **Step 5: Commit**

```bash
git add app/src/components/ps5-launcher/UpdateDialog.tsx app/src/components/desktop/settings/GeneralSection.tsx app/src/i18n
git commit -m "feat(updater): UI do canal empacotado (disponivel/baixando/pronto) nos dois modos"
```

---

### Task 5: Fluxo de release (os yml no lugar certo)

**Files:**
- Modify: `README.md` (seção de release, 66-76) — ou um `scripts/release.md` se o dono preferir

**Interfaces:**
- Consumes: os artefatos da Task 1.
- Produces: checklist do release que garante o par certo de yml.

- [ ] **Step 1: Documentar o checklist**

Escreva, na seção de release do README: (1) `npm run dist:appimage` e `npm run dist:nsis` **da mesma versão**; (2) anexar os 4 assets **+ `release/latest.yml` + `release/latest-linux.yml`** (do build atual — nunca reaproveitar yml antigo da pasta); (3) publicar **não-draft e não-prerelease** (o updater ignora draft/prerelease); (4) `portable`/`zip` saem sem yml, por design.

- [ ] **Step 2: Validar com o release real**

Run: `gh release view v1.4.1 --json assets --jq '.assets[].name' | sort`
Expected (depois do próximo release): os 4 assets + `latest.yml` + `latest-linux.yml`.

- [ ] **Step 3: Commit**

```bash
git add README.md
git commit -m "docs: checklist de release com os yml do updater"
```

---

### Task 6: Validação de ponta a ponta

**Files:**
- Modify: nenhum (validação)

- [ ] **Step 1: Suíte completa e build**

Run: `cd app && npx tsc --noEmit && npm test 2>&1 | tail -5 && npm run build 2>&1 | tail -1`
Expected: 1 falha pré-existente (`steam-path`), resto verde.

- [ ] **Step 2: O canal da fonte não regrediu**

Run: `cd ~/Documents/projects/arcadia && ./arcadia.sh` (alguns segundos) e confira no log que o updater git continua sendo acionado e que o módulo empacotado não faz nada (`isPackaged === false`).

- [ ] **Step 3: Teste manual do canal empacotado (documentado no PR)**

Publicar uma versão de teste (ex.: 1.4.2), abrir o AppImage 1.4.1, ver o diálogo, escolher Baixar, conferir o progresso, escolher Reiniciar e validar a versão nova. Registrar o resultado no PR.

- [ ] **Step 4: Reportar**

No relatório: os números da suíte, o resultado do teste manual e o que ficou fora de escopo (assinatura de código, delta, canais beta).
