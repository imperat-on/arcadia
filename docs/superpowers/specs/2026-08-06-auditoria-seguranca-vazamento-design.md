# Auditoria de Segurança — Vazamento de Dados/Credenciais

**Data:** 2026-08-06
**Projeto:** Arcadia (launcher Electron 33 + React + Supabase free)
**Autor:** Hermes (com aprovação do usuário)
**Status:** Aprovado pelo usuário (design) — aguardando implementação

## Objetivo

Verificar se um **atacante externo** ("alguém atacando o projeto pra ver se captura algo")
consegue obter dados da conta do usuário — credenciais (login/senha), token de sessão,
perfil, biblioteca, horas, conquistas, avatares — por qualquer superfície: servidor
Supabase, cliente Electron, rede, XSS/webview, ou artefatos do projeto (git/build).

Escopo combinado pelo usuário: **abordagens A (superfície + exploração real) + B (red team)
+ C (checklist)** — auditoria completa.

## Histórico relevante

- Auditoria anterior (`arcadia-seguranca-audit-report.md`): 12 vulnerabilidades (6 🔴),
  todas corrigidas no código (commit `75ccdd1` + T2–T8) e no servidor (migração v6
  `docs/sql/migracao-2026-08-05-seguranca-v6.sql` — **já aplicada**: `login_check` ativo).
- Pós-auditoria, achado extra corrigido: `config:set` devolvia chaves de API em claro
  (agora redigido como o `config:get`).
- Features recentes (Running/Stop `game:active`, perfil "Todos os jogos" com horas,
  AuthDialog com `erroKey`, single-instance lock) **nunca passaram por revisão de
  segurança** — alvo desta auditoria.

## Fases

### Fase 1 — Checklist do servidor (v6 completa?)

Verificação estática + probes contra a instância `ztvrvjezklorogrevmhg`:

1. `login_email` **revogado**: chamar o RPC → deve falhar (função não existe / permissão negada).
2. `login_check` ativo: responde `usuario_nao_existe` vs `senha_errada` vs `muitas_tentativas`
   (throttle). Anotar: o par erro-diferente é um **oráculo de enumeração de usernames**
   (aceitável? decidir no relatório).
3. RPCs da v5/v6 — para cada um: security definer? aceita dono arbitrário no payload?
   - `sync_achievements` (push de conquistas: o `user_id` vem de `auth.uid()` ou do payload?)
   - `pull_achievements`, `push_library`, `pull_library` (user_id do payload?)
   - `updatePerfil` / `profile:update` (aceita `id`/`user_id` de outra conta?)
   - `signUp` (criação), `username_available`
4. Tabelas: `profiles` (select público? RLS), `user_library`, `user_playtime` (RLS self-only),
   `achievements` (RLS self-only?).
5. Storage `avatars`: policy de INSERT/UPDATE/DELETE exige dono
   (`storage.foldername(name)[1] = auth.uid()::text`) — confirmar com upload de teste.
6. `profile_visibility` + view `profiles_safe` (public/friends/private) — conferir que o
   fetch de perfil usa a view e que amigos aceitos sempre veem.

### Fase 2 — Red team real (conta `amigoteste` como atacante)

Com a sessão da conta maliciosa (`amigoteste`), tentar de verdade:

1. Ler perfil de `zesmehentperu` (select direto + via RPC de perfil) — com visibilidade
   public / friends / private (testar os 3 cenários).
2. Ler biblioteca/horas/conquistas de outra conta: `pull_library`, `pull_achievements`,
   select em `user_library`/`user_playtime`/`achievements` com filtro em outra conta.
3. Escrever na conta alheia: `push_library` com payload de outra conta; `updatePerfil`
   com id de outra conta; `sync_achievements` com user_id de outra conta.
4. Storage: upload/overwrite/delete de avatar no path de outro usuário
   (`<uid>/<arquivo>` de outra conta).
5. Listar usuários: `select * from profiles` sem filtro (enumerar base de contas).

Cada tentativa: registrar status HTTP + corpo da resposta (evidência).

### Fase 3 — Credenciais no cliente (Electron)

1. `session.json` (token do Supabase): permissões atuais (esperado 0600), conteúdo.
2. `config.json`: **chave de API (hubcap_api_key) em claro** — permissões atuais
   (esperado 644? — outro usuário do Linux leria). Fix se necessário: 0600.
3. Senha: fica cacheada em memória/arquivo após login? (revisar `supabase/auth.js` —
   o objeto de sessão guarda senha? o renderer guarda?).
4. **Token chega no renderer?** Revisar preload.js: alguma API expõe a sessão/token pro
   renderer? Se sim → XSS no renderer = roubo de sessão (🔴). Verificar AccountContext,
   ProfileBridge, AuthDialog, ipc.js (conta restaurada no boot: o token trafega pro
   renderer?).
5. Logs de launch (`logs/<id>.log`): contêm segredos? (comando do jogo — esperado sem
   credenciais; confirmar que env vars com token não são logadas).

### Fase 4 — Rede/transporte

1. Todas as chamadas de rede são HTTPS? (Supabase ✓; `httpfetch.js`: Steam Store, capas,
   wallhaven, PSN — checar URLs http://; o app não faz chamada http:// pro Supabase).
2. Token/senha em URL? (query strings, headers não-padrão).
3. O cliente Supabase usa `persistSession`/localStorage? (onde o token fica no renderer —
   localStorage do Chromium é lido por XSS).

### Fase 5 — XSS / webview

1. Toast de conquista (`notify.html`): usa innerHTML com payload (title/desc/icon)?
   Tem CSP? O payload do toast vem de arquivos locais do Steam (quem controla?) —
   um título de conquista malicioso (ex: jogo com conquista nomeada `<img onerror=...>`)
   executaria no toast? Testar com payload real.
2. Webview da Steam (`webview-steam-preload.js`): o preload expõe o quê pro mundo?
   `postMessage` do webview pro StoreGamePage: o handler valida origem/conteúdo?
3. Notícias (RSS): `limparTexto`/strip ✓ (confirmar), `sanitizeHtml` no GameDetailPanels ✓.
4. CSP do index.html: `script-src 'self'` (sem inline/eval) — confirmar que continua e
   que nenhum código novo (feature work) introduziu inline handlers/script.

### Fase 6 — Repo / artefatos

1. `git grep` por segredos: `hubcap_api_key`, `sb_publishable`, `sk-`, tokens, senhas.
2. `.gitignore`: cobre `config.json`, `session.json`, `contas/`, `sync_state.json`,
   `logs/`? (T2 cobriu parte — reconfirmar incl. novos arquivos).
3. Build (`app/dist/`): contém chaves? (grep).
4. `/tmp`: scripts/probes temporários da sessão atual (removidos ao final).
5. `config.example.json` (se existir): contém chave REAL ou placeholder?

## Critérios de sucesso

- Relatório final com: achados (severidade 🔴🟡🟢), evidência de exploração (status HTTP +
  corpo), e decisão por achado (fixar / aceitar com justificativa).
- Fixes aplicados no código com commit + verificação ad-hoc (padrão do projeto:
  scripts `/tmp/hermes-verify-*.js` executados e removidos).
- Migração SQL v7 **somente se** a Fase 1/2 achar furo no servidor.
- `npm test` (34/34), `tsc --noEmit`, `npm run build` verdes ao final.
- Nenhum dado real do usuário exposto durante os testes (probes usam contas de teste:
  `amigoteste` como atacante; `check-*`/`prova-final-*` existentes).

## Fora de escopo (por decisão do usuário)

- Redesign de UI/UX.
- Mudanças de arquitetura (ex: migrar de Supabase) — só fixes pontuais.
- Performance/memória (já auditado em rodada anterior).

## Entregáveis

1. `docs/superpowers/specs/2026-08-06-auditoria-seguranca-vazamento-design.md` (este arquivo).
2. Relatório de achados (arquivo `.md` em `docs/` ou resposta no chat — decidir na execução).
3. Commits de fix com verificação ad-hoc.
