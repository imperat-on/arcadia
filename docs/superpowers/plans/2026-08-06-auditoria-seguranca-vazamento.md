# Auditoria de Vazamento de Dados/Credenciais — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Provar (com exploração real) que um atacante externo não consegue capturar dados da conta do usuário (credenciais, token, perfil, biblioteca, horas, conquistas, avatares) e corrigir qualquer furo encontrado.

**Architecture:** Auditoria em 6 fases por superfície (servidor → red team → cliente → rede → XSS → repo), cada achado com evidência (status HTTP + corpo), fixes com commits + verificação ad-hoc. Migração SQL v7 apenas se achar furo no servidor.

**Tech Stack:** curl + jq contra a API do Supabase (projeto `ztvrvjezklorogrevmhg`), grep/ripgrep no código, revisão manual de preload/notify/webview, permissões de arquivo.

## Global Constraints

- API base: `https://ztvrvjezklorogrevmhg.supabase.co`; anon key pública: `sb_publishable_7ey2g6CBmmr2KCECuTje9A_b-LN8wlB`
- Contas de teste: `amigoteste` (senha `123456`) = ATACANTE; `zesmehentperu` = VÍTIMA
- Nenhum dado real além de contas de teste durante probes; probes em `/tmp`, removidos ao final
- Padrão de verificação do projeto: scripts `/tmp/hermes-verify-*.js` executados e removidos
- `npm test` (34/34), `tsc --noEmit`, `npm run build` verdes no fim
- Relatório final em PT-BR com severidade 🔴🟡🟢 + evidência + decisão por achado

---

### Task 1: Fase 1 — Checklist do servidor (v6 completa)

**Files:**
- Read: `docs/sql/migracao-2026-08-05-seguranca-v6.sql`, `docs/sql/migracao-2026-08-05-sync-biblioteca.sql`, `docs/sql/schema.sql`
- Create (temporário): `/tmp/fase1-probes.sh`

**Interfaces:**
- Consumes: nada
- Produces: lista de RPCs existentes + quais aceitam dono no payload (para Task 2)

- [ ] **Step 1: Confirmar `login_email` revogado** — `curl -s -o /tmp/r.txt -w "%{http_code}" -X POST ".../rest/v1/rpc/login_email" -H "apikey: $KEY" -H "Content-Type: application/json" -d '{"p_username":"zesmehentperu","p_password":"x"}'` — Esperado: 404 (função não existe) ou 4xx permission denied. 200 = 🔴 ACHADO.
- [ ] **Step 2: Testar throttle do `login_check`** — 6 chamadas seguidas com senha errada → última deve retornar `muitas_tentativas`.
- [ ] **Step 3: Ler a v6/schema e listar RPCs** — `grep -n "create function\|create or replace function" docs/sql/*.sql` — para cada RPC: anotar `security definer` e se o dono vem de `auth.uid()` ou de payload.
- [ ] **Step 4: Probar `sync_achievements`/`pull_achievements`** — chamar sem sessão (anon): deve falhar (auth required). Anotar status.
- [ ] **Step 5: Probar `push_library`/`pull_library`** — idem anon: deve falhar. Anotar.
- [ ] **Step 6: Probar storage** — `GET /storage/v1/object/public/avatars/<uid-de-outrem>/x.png` → 200 esperado (avatares públicos por design). `POST /storage/v1/object/avatars/<uid-de-outrem>/x.png` sem sessão → 401 esperado.
- [ ] **Step 7: Registrar resultados** — anotar achados na Task 7 (relatório).

### Task 2: Fase 2 — Red team real (amigoteste como atacante)

**Files:**
- Create (temporário): `/tmp/fase2-redteam.sh`
- Read: `app/electron/supabase/auth.js` (fluxo de login p/ extrair token)

**Interfaces:**
- Consumes: lista de RPCs da Task 1
- Produces: evidências de exploração (status + corpo) por tentativa

- [ ] **Step 1: Login do atacante** — `POST /auth/v1/token?grant_type=password` com `amigoteste`/`123456` → extrair `access_token` (variável `ATK`).
- [ ] **Step 2: Ler perfil da vítima (select direto)** — `GET /rest/v1/profiles?id=eq.<uid-vitima>` com `Authorization: Bearer $ATK` → registrar o que retorna (RLS deve bloquear; se retornar dados → 🔴).
- [ ] **Step 3: Ler biblioteca/horas da vítima** — `GET /rest/v1/user_library?user_id=eq.<uid-vitima>` e `user_playtime` com $ATK → RLS deve bloquear (200 com 0 linhas = ok; 200 com dados = 🔴).
- [ ] **Step 4: Ler conquistas da vítima** — `GET /rest/v1/achievements?...` (se tabela existir) idem.
- [ ] **Step 5: Escrever na conta alheia** — `POST /rest/v1/rpc/push_library` com payload contendo user_id da vítima → deve falhar (RLS/RPC). Registrar.
- [ ] **Step 6: updatePerfil da vítima** — `POST /rest/v1/rpc/updatePerfil` (ou nome real do RPC) com id da vítima → deve falhar. 200 = 🔴.
- [ ] **Step 7: Sobrescrever avatar da vítima** — `POST /storage/v1/object/avatars/<uid-vitima>/hack.png` com $ATK e body binário → deve dar 403 (policy de dono). 200 = 🔴.
- [ ] **Step 8: Enumerar usuários** — `GET /rest/v1/profiles?select=username,display_name` com $ATK → RLS deve bloquear listagem (0 linhas ou erro). 200 com linhas = 🟡/🔴.
- [ ] **Step 9: Oracle de usernames** — comparar respostas `login_check` para usuário existente vs inexistente (anotar como 🟢 aceitável ou 🟡).
- [ ] **Step 10: Registrar resultados** — para a Task 7.

### Task 3: Fase 3 — Credenciais no cliente

**Files:**
- Read: `app/electron/supabase/auth.js`, `app/electron/supabase/session.js`, `app/electron/supabase/ipc.js`, `app/electron/preload.js`, `app/src/global.d.ts`
- Terminal: verificar permissões dos arquivos

- [ ] **Step 1: Permissões de arquivos** — `ls -la config.json session.json contas/ logs/` no DATA_DIR (`~/.local/share/arcadia/`) — esperado: session 0600, contas 0700. `config.json` 644 com chave em claro → 🟡 fix (chmod 600 no boot).
- [ ] **Step 2: Senha cacheada?** — grep em `auth.js`/`session.js` por campos de senha salvos (ex: `password` em JSON/arquivo). Se a senha persiste em disco → 🔴 fix.
- [ ] **Step 3: Token chega no renderer?** — grep no preload por exposição de sessão/token (ex: `getSession`, `session`, `access_token`). Se o renderer recebe o token → 🔴 (XSS rouba sessão) — fix: só expor status/dados, nunca o token.
- [ ] **Step 4: localStorage do renderer** — grep por `localStorage` com token/sessão. Se o supabase-js persiste sessão no localStorage do renderer → 🟡 (XSS lê) — fix: `persistSession: false` no client do renderer.
- [ ] **Step 5: Logs de launch** — `ls logs/` + `head` de um log: contém segredos? (cmd do jogo, env vars com token?)
- [ ] **Step 6: Registrar resultados.**

### Task 4: Fase 4 — Rede/transporte

**Files:**
- Read: `app/electron/httpfetch.js`, `app/electron/main.js` (grep URLs), `app/electron/supabase/*.js` (URLs)

- [ ] **Step 1: grep de `http://`** — `grep -rn "http://" app/electron/ app/src/ | grep -v "localhost\|127.0.0.1\|example\|w3.org\|schema"` — cada URL http:// = 🟡 (MITM) — anotar se trafega dado sensível.
- [ ] **Step 2: Supabase sempre HTTPS?** — grep por `supabase.co` com `http://`.
- [ ] **Step 3: Token em URL?** — grep por `access_token=`/`token=` em URLs construídas (query strings).
- [ ] **Step 4: Registrar resultados.**

### Task 5: Fase 5 — XSS / webview / toast

**Files:**
- Read: `app/notify.html` (ou onde estiver), `app/electron/notify.js`, `app/electron/webview-steam-preload.js`, `app/index.html` (CSP), `app/electron/news.js`

- [ ] **Step 1: Toast (notify)** — o HTML usa `innerHTML` com title/desc/icon do payload? Tem CSP? Se innerHTML sem sanitização → testar com payload `<img src=x onerror=...>` (probe: rodar o toast com payload malicioso e ver se executa no console — via `showAchievementToast` num probe Electron). 🔴 se executar.
- [ ] **Step 2: Webview da Steam** — o preload do webview expõe APIs pro mundo (contextBridge)? O handler de `postMessage` no StoreGamePage valida origem? Anotar.
- [ ] **Step 3: CSP** — `grep -n "Content-Security-Policy" app/index.html` — `script-src 'self'` sem unsafe-inline/eval ✓; procurar `onClick={...}` inline (React onX = evento JS, não é inline HTML — ok); procurar `dangerouslySetInnerHTML` no src (fora do sanitize).
- [ ] **Step 4: Notícias** — `limparTexto` faz strip de tags? (já revisado antes ✓ — reconfirmar).
- [ ] **Step 5: Registrar resultados.**

### Task 6: Fase 6 — Repo / artefatos

**Files:**
- Terminal: `git grep`, `cat .gitignore`, grep no dist

- [ ] **Step 1: git grep por segredos** — `git grep -nE "hubcap_api_key|sb_publishable|sk-[A-Za-z0-9]{20}|BOTCLAW|GOOGLE_API"` — qualquer hit = 🔴 (remover do histórico? pelo menos do working tree + .gitignore).
- [ ] **Step 2: .gitignore** — conferir: `config.json`, `session.json`, `contas/`, `sync_state.json`, `logs/`, `*.log`, `dist/` (ou build/). Adicionar o que faltar.
- [ ] **Step 3: Build** — `grep -rE "hubcap_api_key|sb_publishable" app/dist/ 2>/dev/null | head` — o bundle contém a chave? (a anon key é pública por design — ok aparecer; a hubcap NÃO pode).
- [ ] **Step 4: config.example.json** — existe? Contém chave real ou placeholder?
- [ ] **Step 5: Registrar resultados.**

### Task 7: Relatório + fixes + validação final

**Files:**
- Create: `docs/auditoria-vazamento-2026-08-06.md` (relatório)
- Fixes conforme achados (código + commit + `/tmp/hermes-verify-*.js`)
- Migração v7 **somente se** Task 1/2 achar furo no servidor

- [ ] **Step 1: Consolidar achados** das Tasks 1-6 em `docs/auditoria-vazamento-2026-08-06.md` (severidade, evidência, decisão).
- [ ] **Step 2: Aplicar fixes de código** (cada um com commit + verificação ad-hoc).
- [ ] **Step 3: Migração SQL v7** se necessário (clipboard via busctl + usuário roda no SQL Editor — MESMO padrão da v6: avisar o usuário).
- [ ] **Step 4: Validação final** — `npm test` (34/34), `tsc --noEmit`, `npm run build`, verificação ad-hoc consolidada.
- [ ] **Step 5: Remover probes** de `/tmp` e commitar o relatório.

## Self-Review

1. **Spec coverage:** Fase 1→Task 1, Fase 2→Task 2, Fase 3→Task 3, Fase 4→Task 4, Fase 5→Task 5, Fase 6→Task 6, entregáveis/critérios→Task 7. ✓
2. **Placeholders:** nenhum TBD — todos os passos têm comandos concretos. ✓
3. **Type consistency:** contas/endpoints consistentes (amigoteste/zesmehentperu, mesma API base). ✓
