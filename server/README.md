# Arcadia Server

Backend Node proprio do Arcadia (substitui o Supabase). Servico unico
(Express + SQLite + JWT) que roda num notebook em casa e exposto via
Tailscale Funnel. Sincroniza por conta: biblioteca, conquistas, horas,
amigos, avatar, background e sources publicas.

## Stack

| Camada | Tecnologia |
|---|---|
| Runtime | Node >= 22 |
| HTTP | Express 4 |
| Banco | SQLite via `node:sqlite` (WAL) |
| Auth | JWT (HS256) + bcryptjs, refresh tokens opacos |
| Realtime | WS (pacote `ws`), protocolo Phoenix-lite |

## Estrutura

```
server/
  package.json
  .env.example        # JWT_SECRET, PORT, DATA_DIR (nunca versione .env)
  src/
    server.js         # bootstrap express + ws + registro de rotas
    db.js             # schema SQLite + seeds + helpers de tempo
    jwt.js            # emit/verify JWT no shape GoTrue
    auth-routes.js    # signup, login, token, user, logout, login_check
    rest-routes.js    # REST-lite de profiles/friendships (RLS explicito)
    sync-routes.js    # RPCs: conquistas, biblioteca, sources
    storage-routes.js # buckets avatars (5MB) e backgrounds (25MB, video)
    realtime.js       # WS Phoenix-lite, canal friends-<me>
  data/               # arcadia.db (gitignored)
  test/               # node --test (53 testes)
```

## Rodar

```bash
cd server
cp .env.example .env    # gere um JWT_SECRET forte: openssl rand -hex 32
npm install
npm start               # ou: node src/server.js
curl localhost:3000/health
```

## Testes

```bash
npm test    # node --test, 53 testes (auth, storage, rpc, realtime, e2e)
```

## Endpoints

### Auth (GoTrue-lite)
| Metodo | Rota | Descricao |
|---|---|---|
| POST | `/auth/v1/signup` | cria conta + devolve sessao |
| POST | `/auth/v1/token?grant_type=password` | login por email+senha |
| POST | `/auth/v1/token?grant_type=refresh_token` | renova sessao |
| GET | `/auth/v1/user` | perfil do token |
| POST | `/auth/v1/logout` | invalida refresh tokens |

### RPCs (todas exigem `Authorization: Bearer <jwt>`)
| Rota | Args | Retorno |
|---|---|---|
| `POST /rest/v1/rpc/login_check` | `{p_username, p_password}` | `{ok, email}` ou `{ok:false, error}` (publico) |
| `POST /rest/v1/rpc/username_available` | `{p_username}` | boolean (publico) |
| `POST /rest/v1/rpc/sync_achievements` | `{p_items}` | rows alteradas (earliest-wins) |
| `POST /rest/v1/rpc/pull_achievements` | `{p_since}` | delta de conquistas |
| `POST /rest/v1/rpc/friend_achievements` | `{p_friend}` | 30 conquistas (4 guardas) |
| `POST /rest/v1/rpc/push_library` | `{p_lib, p_playtime}` | void (upsert + deltas) |
| `POST /rest/v1/rpc/pull_library` | - | library + playtime da conta |
| `POST /rest/v1/rpc/push_sources` | `{p_sources}` | void (soft-delete) |
| `POST /rest/v1/rpc/pull_sources` | - | sources publicas da conta |

### REST-lite (`/rest/v1/:table`)
`profiles` e `friendships`, com filtros `?col=eq.val`, `?col=ilike.pat`,
`?or=(...)`, `?limit=`, `?select=` (com embeds `pa:/pb:`). RLS e aplicado
explicitamente: so ve o que e seu, publico ou amigo.

### Storage (`/storage/v1/object/:bucket/:uid/:file`)
- `avatars`: so imagem, 5MB, `image/*`
- `backgrounds`: imagem ou video (webm/mp4/m4v/mov), 25MB
- Serve publico em `/storage/v1/object/public/:bucket/:uid/:file`
- Upload owner-scoped (uid do path == sub do token), magic bytes validados

### Realtime (`/realtime/v1/websocket`)
Protocolo Phoenix-lite. Canal `friends-<me>`: evento `postgres_changes`
no INSERT de friendships com `user_b=me` (badge de amigos).

## Schema (SQLite)

`profiles`, `friendships`, `user_achievements`, `user_library`,
`user_playtime`, `user_sources`, `login_attempts`, `reserved_usernames`,
`blocks`, `refresh_tokens`. Cria com `CREATE TABLE IF NOT EXISTS` no boot.

## Migracao de schema

**Atencao:** `CREATE TABLE IF NOT EXISTS` cria tabelas novas, mas NAO
adiciona colunas em tabelas existentes. Ao adicionar coluna em producao:

```bash
sqlite3 server/data/arcadia.db "ALTER TABLE profiles ADD COLUMN background_url TEXT;"
systemctl restart arcadia-server
```

## Seguranca

- Senhas com bcrypt (nunca texto plano)
- JWT secret em `.env`, nunca versionado
- RLS substituido por filtro explicito `user_id = sub` em cada query
- Throttle de login (5 falhas/10min)
- Magic bytes validam uploads (nao confia no content-type)
- Sources com API key NUNCA sincronizam (nao ha campo no schema)
- `realName` fica local (privacidade). Background sincroniza
