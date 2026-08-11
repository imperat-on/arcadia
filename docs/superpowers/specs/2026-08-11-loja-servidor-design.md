# Design — Loja no Servidor (proxy de catálogo)

**Data:** 2026-08-11
**Status:** Aprovado
**Plano de implementação:** [2026-08-11-loja-servidor.md](../plans/2026-08-11-loja-servidor.md)
**Objetivo:** o servidor Node (Express+SQLite) vira a **fonte única de catálogo da loja**. O app deixa de buscar catálogo de terceiros (Hydra, SteamSpy, Steam, etc) e consulta o servidor, que baixa uma vez por TTL e responde JSON pronto.

## Escopo

### Vai pro servidor (catálogo público)

| Dado | Hoje no app | TTL hoje | Fonte externa |
|---|---|---|---|
| Catálogo Hydra (sources) | `sources/*.json` (~28MB) | ETag/304 | hydralinks.cloud |
| Lista "Em alta" (popular) | `store_popular_cache.json` | 6h | SteamSpy |
| Tipo + arte por appid | `store_items_cache.json` | 7d | IStoreBrowseService |
| Lista de appids com manifesto | `store_sushi_cache.json` | 6h | GitHub sushi repo |
| Disponibilidade de manifestos | `store_manifest_cache.json` | 7d | 4 provedores |
| Listas alternativas | `store_genre_cache.json` | 12h | SteamSpy etc |
| Requisitos de sistema | `sysinfo_cache.json` | sem TTL | Steam appdetails |
| Metadados de jogo | `meta_cache.json` | sem TTL | Steam + SteamGridDB |
| Notícias | `news_cache.json` | 30min | 6 RSS |
| HowLongToBeat | `hltb_cache.json` | 30d | HLTB |
| Índices de fixes | `cache/fixes-index.json`, `ryuu-index.json` | 6h | luatools.work, ryuu |

### Fica local (por política e decisão)

- **Trailers** (vídeos pesados, decisão explícita do usuário)
- **API keys pagas do usuário**: Hubcap, Real-Debrid, TorBox, AllDebrid, Premiumize, Steam key — servidor nunca conhece chaves de terceiros
- `library.json`, `game_settings.json`, `downloads.json`, `art/`, `bin/`, `logs/`, `session.json`

### Acessos diretos que continuam no app

- Manifestos com token (Hubcap) — não é catálogo, é instalação
- Instalação DepotDownloader
- Busca de títulos via `store:suggest` (Steam sugestão é leve)
- Trailers

## Arquitetura

```
App ──HTTP──> Servidor ──fetch 1x/TTL──> Hydra/SteamSpy/Steam (no notebook)
   ^              |
   └── responde JSON do catálogo ──┘
```

### Storage: tabela genérica `catalog_cache` (SQLite)

```sql
CREATE TABLE IF NOT EXISTS catalog_cache (
  key  TEXT PRIMARY KEY,
  data TEXT NOT NULL,
  at   INTEGER NOT NULL
);
```

- **Segurança:** allowlist de keys no servidor (nada de path traversal); JWT obrigatório (como o resto)
- **Atomicidade:** `INSERT OR REPLACE` (sem `.tmp`+rename)
- **Backup:** coberto pelo `arcadia-backup.sh` existente (WAL-safe)
- **Genérico:** 13 tipos numa tabela; adicionar cache = um INSERT

### Endpoints (`/catalog/v1/*`, todos exigem `Authorization: Bearer <jwt>`)

| Endpoint | Devolve |
|---|---|
| `GET /catalog/v1/sources` | lista de fontes Hydra registradas + metadados |
| `GET /catalog/v1/sources/:id/games` | JSON completo de uma fonte (com `uris`) |
| `GET /catalog/v1/search?q=` | busca no catálogo Hydra (índice do servidor) |
| `GET /catalog/v1/popular?lista=&limite=&offset=` | "Em alta" (SteamSpy) |
| `GET /catalog/v1/items?appids=` | tipo + arte por appid |
| `GET /catalog/v1/sushi` | appids com manifesto |
| `GET /catalog/v1/manifests/:appid` | disponibilidade de manifestos (sondagem) |
| `GET /catalog/v1/genre?lista=` | listas alternativas |
| `GET /catalog/v1/sysinfo/:appid` | requisitos de sistema |
| `GET /catalog/v1/meta/:appid` | metadados de jogo |
| `GET /catalog/v1/news` | notícias (RSS) |
| `GET /catalog/v1/hltb/:appid` | HowLongToBeat |
| `GET /catalog/v1/fixes` | índices de fixes |

Respostas com gzip (`compression`). TTLs iguais aos do app.

## Fluxo de dados

1. Renderer chama o IPC existente (`store:search`, `store:recent`, `store:installInfo`, etc).
2. Main process chama `GET /catalog/v1/*` via `fetchRede` (httpfetch.js).
3. Servidor: cache válido → responde direto; cache vencido → **stale-while-revalidate** (devolve o que tem, busca em background); sem cache → busca da fonte externa.
4. App atualiza **espelho local** (o cache antigo) com a resposta e devolve ao renderer.
5. Servidor fora/timeout → app devolve o espelho (loja continua abrindo). Sem espelho ainda → estado vazio + aviso.

## Offline-first

- Espelho local = último estado recebido do servidor (arquivos antigos servem de fallback)
- Servidor é fonte, local é fallback — zero regressão de disponibilidade
- Sem rede nenhuma → "Loja indisponível offline" (melhor que travar)

## Erros

- Servidor: `401` (sem JWT), `400` (key inválida), `404` (cache ainda não buscado), `502` (fonte externa falhou sem cache)
- App: retry com backoff (padrão `isRetryable`), timeout, queda pro espelho

## Testes

- Servidor (`node --test`): 200 com cache, 200 stale, 401, 404, allowlist, fetch externo mockado
- App: fallback (servidor fora → espelho), busca delegada
- E2E: servidor sobe, app consulta, catálogo aparece

## Migração (zero quebra)

- `catalog_cache` criada com `CREATE TABLE IF NOT EXISTS` no boot (padrão do projeto)
- App novo → fala com servidor, cai pro espelho
- App antigo → continua lendo disco, funciona normal (servidor é aditivo)
- JWT existente vale para `/catalog/v1/*` (nada de chave nova)
