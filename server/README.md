# Arcadia Server

Backend Node proprio do Arcadia (substitui o Supabase). Servico unico
(Express + PostgreSQL + JWT) que roda num notebook em casa e exposto via
Tailscale Funnel. Sincroniza por conta: biblioteca, conquistas, horas,
amigos, avatar, background e sources publicas.

## Stack

| Camada | Tecnologia |
|---|---|
| Runtime | Node >= 22 |
| HTTP | Express 4 |
| Banco | PostgreSQL via `pg` |
| Auth | JWT (HS256) + bcryptjs, refresh tokens opacos |
| Realtime | WS (pacote `ws`), protocolo Phoenix-lite |

## Estrutura

```
server/
  package.json
  .env.example        # JWT_SECRET, PORT, DATA_DIR (nunca versione .env)
  src/
    server.js         # bootstrap express + ws + registro de rotas
    db.js             # pool, migrations, seeds + helpers de tempo
    migrations.js      # baseline schema.sql + migrations versionadas
    jwt.js            # emit/verify JWT no shape GoTrue
    auth-routes.js    # signup, login, token, user, logout, login_check
    rest-routes.js    # REST-lite de profiles/friendships (RLS explicito)
    sync-routes.js    # RPCs: conquistas, biblioteca, sources
    storage-routes.js # buckets avatars (5MB) e backgrounds (25MB, video)
    community-validation.js # normalizacao e limites sem dependencias
    community-routes.js # reviews/avaliacoes, colecoes/listas, reports e moderacao
    realtime.js       # WS Phoenix-lite, canal friends-<me>
  data/               # arcadia.db (gitignored)
  test/               # node --test (suíte unitária, integração e e2e)
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
npm test    # node --test (auth, storage, rpc, realtime, catalogo e e2e)
```

A suíte precisa de PostgreSQL. Para subir um banco descartável localmente:

```bash
docker compose -f docker-compose.test.yml up -d
TEST_DATABASE_URL=postgres://arcadia:arcadia@127.0.0.1:55432/arcadia_test npm test
docker compose -f docker-compose.test.yml down -v
```

O workflow `.github/workflows/ci.yml` executa os testes com PostgreSQL 16.

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

### Observabilidade, erros e readiness

Todas as respostas HTTP recebem `X-Request-Id`. O cliente pode enviar um
identificador seguro nesse mesmo header (ate 128 caracteres); se ausente ou
invalido, o servidor gera um UUID. O header tambem e exposto ao renderer pelo
CORS para correlacionar uma falha com os logs do servidor.

Respostas de erro HTTP mantem a chave legada `error` para clientes existentes e
agora incluem campos estruturados: `code` (igual a `error`), `message`
generica e `request_id` (igual ao header). Exemplo:

```json
{
  "error": "nao_autenticado",
  "code": "nao_autenticado",
  "message": "Autenticacao necessaria",
  "request_id": "desktop-42"
}
```

`GET /health` preserva o contrato anterior e verifica o processo + banco.
`GET /ready` e o probe de readiness para deploy/orquestrador: responde `200`
com `{"ok":true,"ready":true}` quando o PostgreSQL esta acessivel e `503`
com erro estruturado quando nao esta.

### Realtime (`/realtime/v1/websocket`)
Protocolo Phoenix-lite. Canal `friends-<me>`: evento `postgres_changes`
no INSERT de friendships com `user_b=me` (badge de amigos).

## Comunidade

Reviews ficam em `user_reviews` (uma por conta/jogo), com escala `rating` de 1 a
5 e status de moderacao. Colecoes usam `collections` + `collection_items` e
aceitam `visibility=public|unlisted|private`; somente colecoes publicas e
visiveis aparecem na descoberta. Reports entram em `community_reports` e a
fila de moderacao exige uma conta em `community_moderators` ou um id listado em
`COMMUNITY_MODERATOR_IDS` (tambem ha `COMMUNITY_MODERATOR_USERNAMES` para
instalacoes simples).

Rotas principais (aliases em portugues `/avaliacoes` e `/listas`):

- `GET/POST /community/v1/reviews[/:appid]`
- `GET/PATCH/DELETE /community/v1/review/:id` e `POST .../report`
- `GET/POST /community/v1/collections`
- `GET/PATCH/DELETE /community/v1/collections/:id`
- `POST/PUT/DELETE /community/v1/collections/:id/items[/:appid]`
- `POST .../review/:id/report` e `POST .../collections/:id/report`
- `GET/PATCH /community/v1/moderation/{reviews,collections}/:id`

Respostas de erro mantem `error` e incluem `code`, `message`, `request_id`; os
limites de payload sao validados antes das queries (review 4.000 caracteres,
colecao 500 itens, pagina 100).

## Schema (PostgreSQL)

`profiles`, `friendships`, `user_achievements`, `user_library`,
`user_playtime`, `user_sources`, `login_attempts`, `reserved_usernames`,
`blocks`, `refresh_tokens`, `user_reviews`, `collections`, `collection_items`,
`community_reports`, `community_moderators`. Cria com `CREATE TABLE IF NOT
EXISTS` no boot.

## Migração de schema

O boot executa migrations versionadas dentro de transações e registra cada
versão em `schema_migrations`. O `server/sql/schema.sql` é a baseline v1 para
instalações antigas; mudanças novas devem entrar em
`server/sql/migrations/000X_nome.sql` e nunca ser aplicadas manualmente em
produção.

Para aplicar migrations sem iniciar o servidor, use o mesmo banco configurado
em `DATABASE_URL`:

```bash
npm run migrate:postgres
npm run verify:postgres
```

As migrations usam lock advisory e checksum. Se um arquivo já aplicado for
alterado, o boot falha deliberadamente para evitar drift silencioso. Os scripts
`migrate:sqlite` e `verify:sqlite-migration` ficam separados para a migração
histórica de bases SQLite; não são necessários em um deploy PostgreSQL novo.

## Seguranca

- Senhas com bcrypt (nunca texto plano)
- JWT secret em `.env`, nunca versionado
- RLS substituido por filtro explicito `user_id = sub` em cada query
- Throttle de login (5 falhas/10min por conta) + limite por IP nos endpoints publicos de auth
- Refresh tokens são rotacionados em transação com lock; um token usado não pode ser reaproveitado
- Magic bytes validam uploads (nao confia no content-type)
- Sources com API key NUNCA sincronizam (nao ha campo no schema)
- `realName` fica local (privacidade). Background sincroniza
