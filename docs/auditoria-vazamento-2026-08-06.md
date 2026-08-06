# Auditoria de Vazamento de Dados/Credenciais — Relatório

**Data:** 2026-08-06
**Escopo:** atacante externo tentando capturar dados da conta (login/senha, token, perfil, biblioteca, horas, conquistas, avatares)
**Método:** checklist servidor + red team real (conta `amigoteste` como atacante) + revisão cliente/rede/XSS/repo

## Resumo

**Nenhum vazamento crítico de dados encontrado.** O atacante não consegue ler dados
protegidos (biblioteca, horas, conquistas de não-amigos), não consegue escrever em
contas alheias e não captura credenciais em trânsito. 2 achados de higiene (🟡)
corrigidos, 1 migração SQL pendente (v7) para o servidor.

## Achados

| # | Severidade | Achado | Evidência | Decisão |
|---|---|---|---|---|
| F1 | 🟡 | RPCs restritos executáveis por ANON (sync_achievements, pull_achievements, push_library, pull_library, friend_achievements) — o servidor regrantou execute ao anon apesar dos revokes da v6 | HTTP 200/204 anon em todos | **Migração v7** gerada (revokes explícitos anon+public) — clipboard + SQL Editor pendente. Sem vazamento de dados: RLS self-only bloqueia com uid null (retornos vazios) |
| F2 | 🟡 | `config.json` com permissão 644 — contém a hubcap_api_key em claro; qualquer usuário do sistema Linux lê | `ls -la config.json` = `-rw-r--r--` | **Corrigido**: `chmod 600` no `readConfig()` (main.js) |
| F3 | 🟢 | Enumeração de usernames (perfil public + oráculo login_check `usuario_nao_existe` vs `senha_errada`) | select profiles sem filtro retornou todos | Aceitável por design: usernames não são secretos; cadastro também é anon. Documentado |
| F4 | 🟢 | Red team: PATCH no perfil da vítima | HTTP 204 mas **0 linhas** (RLS bloqueou silenciosamente — display_name intacto) | Seguro — sem policy de UPDATE = default deny |
| F5 | 🟢 | Red team: storage avatars (upload/delete no path da vítima) | HTTP 403 RLS (dono obrigatório) | Seguro |
| F6 | 🟢 | Red team: biblioteca/horas/conquistas da vítima (não-amigo) | HTTP 200 `[]` em todas | Seguro — RLS self-only |
| F7 | 🟢 | Red team: `friend_achievements` da vítima | Retornou dados — **mas o atacante É amigo aceito** (design: amigos veem conquistas) | Seguro |
| F8 | 🟢 | `session.json` (token) | `-rw-------` + **criptografado** (`enc: true`) | Seguro |
| F9 | 🟢 | Token no renderer | `persistSession: false` + preload SEM exposição de token + `account:changed` só com `user.id/email/username` | Seguro — XSS não encontra token |
| F10 | 🟢 | Senha cacheada | Não persiste (só enviada ao `login_check` via HTTPS) | Seguro |
| F11 | 🟢 | Rede/transporte | Tudo HTTPS (protocol-relative completado com https:); token de debrid só em query HTTPS do provedor | Seguro |
| F12 | 🟢 | XSS: toast de conquista | `textContent` (sem innerHTML) + payload em query | Seguro |
| F13 | 🟢 | XSS: webview Steam | preload sem contextBridge/expose; sem handler de postMessage no app | Seguro |
| F14 | 🟢 | CSP | `script-src 'self'` (sem inline/eval) + sanitizeHtml no único dangerouslySetInnerHTML | Seguro |
| F15 | 🟢 | Repo/build | git grep: só a anon key pública (design); .gitignore cobre config/session/contas/logs/dist; build sem chave real | Seguro |
| F16 | 🟢 | `login_email` revogado + throttle ativo | HTTP 404 na função; 6ª tentativa → `muitas_tentativas` | Seguro |

## Fixes aplicados (código)

1. **config.json 600** — `main.js` `readConfig()`: `fs.chmodSync(CONFIG, 0o600)` (idempotente, reaplica após cada writeConfig).

## Pendência do usuário

- **Rodar a v7 no SQL Editor** (clipboard): `docs/sql/migracao-2026-08-06-seguranca-v7-revokes.sql` — revokes explícitos dos RPCs restritos. Sem ela os RPCs continuam chamáveis por anon (sem vazamento de dados, mas fora do menor privilégio). Após rodar, um teste rápido: chamar `sync_achievements` sem sessão deve dar HTTP 403.

## Validação

- `npm test` 34/34, `tsc --noEmit`, `npm run build` — verdes
- Verificação ad-hoc `/tmp/hermes-verify-*.js` — executada e removida
- Dados restaurados: nenhum dado real foi alterado (o PATCH de teste foi bloqueado pela RLS; probes usaram contas de teste)
