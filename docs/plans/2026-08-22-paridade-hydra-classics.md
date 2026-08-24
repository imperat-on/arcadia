# Plano de implementação: paridade com o Hydra Classics

**Data:** 2026-08-22  
**Status:** pronto para implementação  
**Base analisada:** `/home/zes/Documents/projects/hydra`  
**Destino:** `/home/zes/Documents/projects/arcadia`  
**Dependência:** Retro Catalog V2 e catálogo server-first já existentes

## Resultado desejado

A Loja Retro do Arcadia deve seguir o mesmo princípio estrutural do Hydra Classics:

- o servidor mantém um catálogo canônico de jogos, com ID estável, título editorial em inglês, plataforma, gêneros e imagens;
- as Hydra Sources são indexadas no servidor e viram **ofertas ligadas ao jogo canônico**;
- a busca devolve cards já completos, inclusive capa e nomes das sources que possuem download;
- o app não tenta identificar jogo, traduzir título ou procurar capa durante a renderização;
- dados locais são cache e fallback; o servidor é a autoridade, como já ocorre na Store normal;
- URI/magnet só é entregue ao escolher uma oferta, nunca na listagem.

O ganho esperado é eliminar os nomes de releases russos da vitrine, reduzir duplicatas e fazer a Store Retro responder com latência semelhante à Store normal.

## O que o Hydra realmente faz

### Catálogo Classics

No launcher analisado, Classics é uma variação do catálogo normal. O renderer chama:

```http
POST /catalogue/search
Content-Type: application/json

{
  "shops": ["launchbox"],
  "platforms": [],
  "title": "...",
  "downloadSourceIds": ["..."],
  "downloadSourceFingerprints": ["..."],
  "take": 24,
  "skip": 0
}
```

Cada resultado já contém:

```ts
{
  objectId: string
  shop: "launchbox"
  title: string
  platform?: string
  genres: string[]
  releaseYear: number | null
  libraryImageUrl: string | null
  downloadSources: string[]
}
```

O card apenas renderiza `libraryImageUrl`, título, plataforma, gênero e badges de sources. Não existe uma busca de capa por card.

Referências no Hydra:

- `src/renderer/src/pages/catalogue/catalogue.tsx` — busca paginada com `shops: ["launchbox"]`;
- `src/types/index.ts` — contrato `CatalogueSearchResult`;
- `src/renderer/src/pages/catalogue/game-item-classics.tsx` — card e badges de source;
- `src/renderer/src/hooks/use-launchbox-filters.ts` — filtros carregados uma vez pelo servidor;
- `src/big-picture/src/pages/catalogue/use-catalogue-data.ts` — mesmo contrato no Big Picture.

### Sources

Ao adicionar uma source, o Hydra envia a URL ao backend em `POST /download-sources`. O servidor devolve uma source com ID/fingerprint e posteriormente recebe seus IDs em `/catalogue/search`. Assim, o backend pode responder quais das sources configuradas possuem oferta para cada jogo.

O launcher persiste apenas o registro sincronizado da source e seus metadados. Também consulta `/download-sources/changes` para descobrir novas opções de download de jogos que já estão na biblioteca.

Referências:

- `src/main/events/download-sources/add-download-source.ts`;
- `src/main/events/download-sources/sync-download-sources.ts`;
- `src/main/services/download-sources-checker.ts`.

### Identidade por SKU nos jogos locais

Para importar ROMs, o Hydra evita título sempre que possível:

1. varre a pasta e detecta arquivos válidos para o sistema;
2. extrai o SKU/serial do disco ou usa o `games.yml` do RPCS3;
3. normaliza o SKU para maiúsculas e remove caracteres não alfanuméricos;
4. consulta até 100 SKUs por chamada em `POST /games/shop-details`;
5. recebe `objectId`, título, plataforma, metadados e assets do jogo LaunchBox;
6. valida se a plataforma retornada corresponde ao emulador;
7. agrupa discos do mesmo jogo e persiste o vínculo por `launchbox:objectId`.

Título de arquivo só é usado para agrupar discos irmãos; não é a autoridade editorial.

Referências:

- `src/main/events/emulators/import-launchbox-roms.ts`;
- `src/main/services/emulators/launchbox-shop-details.ts`;
- `src/main/events/catalogue/get-game-shop-details.ts`.

### Cache e imagens

O servidor devolve as URLs das imagens na própria busca. O launcher mantém cache local de detalhes e assets por `shop:objectId`, inclusive por idioma para descrições. Imagens animadas podem ser convertidas uma única vez em poster WebP 400×600 e reutilizadas.

O repositório do launcher **não contém o backend do Hydra**. Portanto, ele mostra com precisão o contrato consumido, mas não revela como o servidor privado importa/licencia a base LaunchBox nem como relaciona internamente cada item das sources. O Arcadia não deve copiar dados LaunchBox sem antes confirmar licença e forma de distribuição.

## Diferença entre o Hydra e o Arcadia atual

| Área | Hydra Classics | Arcadia atual | Mudança necessária |
|---|---|---|---|
| Autoridade | catálogo remoto `launchbox` | catálogo remoto derivado dos títulos das sources | introduzir provedor canônico estável |
| Identidade | `shop + objectId`; SKU para ROM | plataforma + título normalizado/hash | SKU/serial/hash primeiro; título apenas fallback |
| Título | editorial/canônico | limpeza do título da oferta | título inglês vindo do catálogo canônico |
| Capa | pronta em `libraryImageUrl` | Libretro/source, com lacunas | materializar e servir capa otimizada no servidor |
| Sources | indexadas no backend e anexadas ao resultado | feeds globais reconstruídos no sync | cadastro/sync server-side por source/fingerprint |
| Renderer | somente apresenta resultado | ainda possui fallbacks de arte/matching | remover trabalho de rede por card |
| Detalhes | cache por ID e idioma | detalhe básico + ofertas | endpoint canônico rico e cache versionado |
| Privacidade | download resolvido sob demanda | URI guardada no catálogo do servidor | separar metadados da oferta e segredo de download |

## Arquitetura alvo

```text
Catálogo canônico autorizado                 Hydra Sources configuradas
(provider ID, inglês, SKU, assets)            (URL -> fingerprint estável)
                 |                                         |
                 v                                         v
        canonical_catalog_import                   source_ingestion
                 |                                         |
                 +----------------+------------------------+
                                  v
                         offer_matching_queue
                    SKU/serial/hash -> alias -> título
                                  |
                                  v
              retro_games <- retro_game_offers -> retro_offers
                    |                 |
                    +--------+--------+
                             v
                catálogo materializado/versionado
                    título + cover CDN + badges
                             |
                  GET /catalog/v2/retro/games
                             |
                      cache local do app
```

## Decisões obrigatórias

### 1. Catálogo canônico independente das ofertas

Não criar um jogo a partir de cada título encontrado nas sources. Importar previamente uma base canônica por plataforma e manter um adaptador de provedores:

```ts
interface RetroMetadataProvider {
  name: string
  importSnapshot(): AsyncIterable<CanonicalGameInput>
  resolveBySkus(skus: string[], locale: "en"): Promise<CanonicalGameInput[]>
  fetchDetails(providerId: string, locale: string): Promise<GameDetails>
}
```

Prioridade recomendada:

1. snapshot/API LaunchBox, **somente se o uso e a redistribuição forem autorizados**;
2. base aberta composta por Libretro Database + Libretro Thumbnails;
3. IGDB ou outro provedor licenciado para completar títulos e assets;
4. overrides editoriais do Arcadia.

O contrato interno deve usar `provider` e `provider_game_id`, mesmo que o primeiro deploy continue usando Libretro. Isso permite atingir paridade funcional agora e trocar/completar o dataset depois sem alterar o app.

### 2. Inglês como título editorial da vitrine

Cada jogo deve possuir:

- `canonical_title_en`, obrigatório para publicação;
- `alternate_titles`, incluindo títulos regionais e nomes das ofertas;
- `localized_titles`, opcional, para futura seleção de idioma;
- `sort_title_en` e `search_document` materializados.

Uma oferta russa pode ser anexada a um jogo inglês, mas seu título original só aparece no seletor de download. Um item sem correspondência canônica não entra na vitrine pública; fica na fila de revisão.

### 3. Reconhecimento das sources no servidor

O fluxo deve ser:

```text
adicionar URL -> validar -> fingerprint -> salvar source -> baixar feed
-> normalizar ofertas -> identificar plataforma -> extrair identificadores
-> resolver jogo canônico -> publicar associação -> incrementar versão
```

O fingerprint deve ser independente do nome exibido e estável entre sincronizações. Sugestão:

```text
sha256(canonical_url + public_key_or_feed_signature)
```

Para feeds legados sem assinatura, usar a URL canônica normalizada e registrar o risco de mudança de domínio.

### 4. Ordem do matching

Usar a seguinte hierarquia, sempre limitada pela plataforma:

1. SKU/serial exato normalizado — confiança 100;
2. SHA-1/MD5/CRC exato — confiança 100;
3. provider ID declarado e validado — confiança 100;
4. alias exato + plataforma — confiança 90;
5. título inglês normalizado + plataforma + região/ano — confiança 82–89;
6. fuzzy com margem clara sobre o segundo candidato — somente fila de revisão, nunca autopublicar abaixo de 82.

Packs, BIOS, DLC, updates, traduções soltas e coleções não devem virar automaticamente jogos individuais. Multi-disc deve compartilhar `game_id` e manter discos/arquivos dentro da oferta.

### 5. Capas servidas pelo Arcadia

URLs externas do Libretro/IGDB não devem chegar diretamente aos milhares de cards. Criar um pipeline:

1. selecionar a melhor arte por prioridade editorial;
2. baixar e validar tipo, dimensões e tamanho;
3. recortar com `contain`, preservando a capa inteira;
4. gerar WebP/AVIF em 320×480 e 640×960;
5. salvar em object storage/CDN com chave imutável pelo hash do conteúdo;
6. persistir `cover_320_url`, `cover_640_url`, `blurhash` e `dominant_color`;
7. definir `Cache-Control: public, max-age=31536000, immutable`;
8. usar placeholder local imediato quando não houver arte.

O catálogo devolve a URL CDN já pronta. O renderer usa `loading="lazy"`, reserva a proporção 2:3 e não executa fallback remoto em cascata.

### 6. Privacidade das URIs

Para alcançar o modelo server-first sem expor segredos:

- listagem e detalhes devolvem somente resumo das ofertas;
- `POST /catalog/v2/retro/offers/:id/resolve` devolve URI com autorização e TTL curto;
- quando a source é pública, o servidor pode reprocessar o feed e resolver sob demanda;
- quando a source é pessoal/privada, armazenar credenciais cifradas por usuário ou manter resolução local;
- magnet/URI nunca entra no espelho local do catálogo nem em logs.

## Modelo de dados V3

Manter as tabelas V2 durante a migração e adicionar:

```sql
CREATE TABLE retro_canonical_games (
  id                  text PRIMARY KEY,
  provider            text NOT NULL,
  provider_game_id    text NOT NULL,
  system_id           text NOT NULL,
  canonical_title_en  text NOT NULL,
  sort_title_en       text NOT NULL,
  alternate_titles    jsonb NOT NULL DEFAULT '[]',
  localized_titles    jsonb NOT NULL DEFAULT '{}',
  release_date        date,
  release_year        integer,
  genres              jsonb NOT NULL DEFAULT '[]',
  developers          jsonb NOT NULL DEFAULT '[]',
  publishers          jsonb NOT NULL DEFAULT '[]',
  description_en      text,
  identifiers         jsonb NOT NULL DEFAULT '{}',
  artwork             jsonb NOT NULL DEFAULT '{}',
  search_document     text NOT NULL,
  metadata_updated_at timestamptz NOT NULL,
  UNIQUE (provider, provider_game_id)
);

CREATE TABLE retro_sources (
  id               text PRIMARY KEY,
  owner_id         text,
  fingerprint      text NOT NULL,
  canonical_url    text NOT NULL,
  display_name     text NOT NULL,
  visibility       text NOT NULL,
  sync_status      text NOT NULL,
  etag             text,
  last_modified    text,
  last_synced_at   timestamptz,
  UNIQUE (owner_id, fingerprint)
);

CREATE TABLE retro_offers_v3 (
  id                text PRIMARY KEY,
  source_id         text NOT NULL REFERENCES retro_sources(id),
  source_item_key   text NOT NULL,
  original_title    text NOT NULL,
  system_id         text,
  identifiers       jsonb NOT NULL DEFAULT '{}',
  metadata          jsonb NOT NULL DEFAULT '{}',
  secret_locator    text,
  content_hash      text NOT NULL,
  first_seen_at     timestamptz NOT NULL,
  last_seen_at      timestamptz NOT NULL,
  removed_at        timestamptz,
  UNIQUE (source_id, source_item_key)
);

CREATE TABLE retro_game_offers (
  game_id          text NOT NULL REFERENCES retro_canonical_games(id),
  offer_id         text NOT NULL REFERENCES retro_offers_v3(id),
  match_method     text NOT NULL,
  confidence       integer NOT NULL,
  matcher_version  text NOT NULL,
  verified_at      timestamptz,
  PRIMARY KEY (game_id, offer_id)
);

CREATE TABLE retro_match_overrides (
  offer_id        text PRIMARY KEY REFERENCES retro_offers_v3(id),
  game_id         text REFERENCES retro_canonical_games(id),
  action          text NOT NULL,
  reason          text,
  actor_id        text,
  updated_at      timestamptz NOT NULL
);
```

Adicionar índices por `(system_id, sort_title_en, id)`, GIN/trigram no documento de busca, `identifiers`, `(source_id, removed_at)` e `(game_id)` nas associações.

## Contrato HTTP V2

### Busca

```http
GET /catalog/v2/retro/games?q=&systems=sony-playstation-2&sourceIds=&limit=24&cursor=
```

Resposta:

```json
{
  "version": "...",
  "items": [{
    "id": "launchbox:1234",
    "title": "Shadow of the Colossus",
    "systemId": "sony-playstation-2",
    "platform": "PlayStation 2",
    "releaseYear": 2005,
    "genres": ["Action", "Adventure"],
    "cover": {
      "url": "https://cdn.../sha256-320.webp",
      "url2x": "https://cdn.../sha256-640.webp",
      "blurhash": "..."
    },
    "offerCount": 3,
    "downloadSources": ["Source A", "Source B"]
  }],
  "nextCursor": "...",
  "total": 18000
}
```

### Filtros

```http
GET /catalog/v2/retro/filters
```

Retorna plataformas, gêneros, publishers, developers e sources já agregados. Cache de 10 minutos no cliente e ETag no servidor.

### Detalhe

```http
GET /catalog/v2/retro/games/:gameId?locale=en
```

Retorna metadados editoriais, assets e resumos das ofertas, sem URI.

### Resolver oferta

```http
POST /catalog/v2/retro/offers/:offerId/resolve
```

Retorna o payload necessário ao downloader somente no clique do usuário.

### Cadastro e sincronização de source

```http
POST /sources/retro
POST /sources/retro/sync
GET  /sources/retro/:id/status
GET  /sources/retro/changes?since=...
```

O app envia URLs/IDs; o servidor realiza ingestão e matching. Para compatibilidade, as rotas atuais continuam disponíveis até o V3 estar completo.

## Plano de execução

### Fase 0 — decisão legal e dataset de referência

- confirmar por escrito se LaunchBox permite ingestão, cache, transformação e redistribuição pelo servidor Arcadia;
- documentar origem, versão e licença de cada campo e imagem;
- se não houver autorização, implementar o mesmo contrato com Libretro + outro provedor autorizado;
- congelar um snapshot de teste com pelo menos 100 jogos por plataforma.

**Saída:** decisão `launchbox` ou `open_catalog`, registrada na configuração do provider.  
**Bloqueio:** produção não pode publicar um snapshot cuja licença não esteja validada.

### Fase 1 — catálogo canônico e identificadores

- criar migração V3 e repositórios;
- implementar adaptador `RetroMetadataProvider`;
- importar título inglês, aliases, plataforma, SKU/serial/hash e metadados;
- criar IDs estáveis `provider:provider_game_id`;
- impedir publicação sem título inglês e plataforma válida;
- manter o V2 ativo enquanto o V3 é construído.

**Aceite:** o mesmo snapshot produz os mesmos IDs em rebuilds; zero título cirílico no campo editorial inglês.

### Fase 2 — Sources server-side

- mover cadastro/sync das sources Retro para o servidor;
- gerar fingerprint estável;
- usar ETag/Last-Modified e cache do último feed válido;
- ingerir incrementalmente e marcar itens removidos sem apagar histórico imediatamente;
- separar `secret_locator` dos metadados públicos;
- registrar métricas por source: duração, itens, alterados, erros e idade do cache.

**Aceite:** source inalterada não força rebuild completo; falha temporária conserva a última versão válida.

### Fase 3 — matcher determinístico

- extrair serial/SKU dos campos e nomes de releases quando confiável;
- implementar lookup em lote, no estilo do Hydra, com chunks de 100;
- adicionar índices por SKU/hash/alias;
- aplicar a ordem de matching definida neste documento;
- versionar algoritmo e confiança;
- criar fila `unmatched/ambiguous` e overrides persistentes;
- reprocessar somente ofertas afetadas por nova source, novo catálogo ou nova versão do matcher.

**Aceite:** nenhum match fuzzy abaixo de 82 é publicado; override sobrevive a todo rebuild; uma oferta pertence a no máximo um jogo ativo.

### Fase 4 — pipeline de artwork

- criar worker e fila de assets;
- importar arte do provider escolhido;
- validar/normalizar e gerar tamanhos 1x/2x;
- publicar em CDN/object storage por content hash;
- armazenar resultado negativo para evitar repetição;
- preaquecer as capas das primeiras páginas e dos jogos com oferta;
- manter Libretro como fallback, mas consumido pelo worker, não pelo renderer.

**Aceite:** resposta da busca não depende de terceiros; p95 de imagem em cache quente abaixo de 150 ms na rede local; layout não muda quando a capa carrega.

### Fase 5 — API de catálogo rápida

- implementar busca cursor-based com índices;
- agregar `offerCount` e `downloadSources` em view/materialized table durante publicação;
- gerar versão imutável e realizar troca atômica do catálogo ativo;
- implementar ETag, compressão Brotli/Gzip e `stale-while-revalidate`;
- incluir filtros pré-agregados;
- limitar payload da página aos campos do card.

**Aceite:** p95 do endpoint de listagem abaixo de 100 ms no servidor e payload de 24 cards abaixo de 100 KB comprimido.

### Fase 6 — app e UI

- adaptar `app/electron/retro-server-catalog.js` ao contrato V2;
- manter espelho local da última página, detalhes usados e manifest;
- remover enriquecimento de capa por card em `retroArtwork.ts`;
- renderizar capa CDN com proporção reservada, lazy loading e placeholder imediato;
- mostrar badges de sources vindos no próprio resultado;
- carregar detalhes por ID e resolver download somente após seleção da oferta;
- preservar fallback local quando o servidor estiver indisponível.

**Aceite:** primeira tela utiliza no máximo uma requisição de catálogo e requisições de imagens; nenhuma busca de metadata individual aparece no waterfall.

### Fase 7 — importação da biblioteca local

- reutilizar o registry de emuladores do Arcadia para scan por sistema;
- extrair SKU/serial de PS1, PS2, PS3, PSP, GameCube e Wii;
- usar hash compatível com bases Libretro para cartuchos;
- resolver identificadores em lote no servidor;
- agrupar multi-disc e persistir `game_id` canônico;
- usar título de arquivo apenas como fallback manual.

**Aceite:** jogos reconhecidos recebem exatamente o mesmo ID usado na Store Retro.

### Fase 8 — revisão manual e operação

- criar tela administrativa para unmatched, ambiguidades, capas faltantes e overrides;
- expor health/readiness da sincronização e filas;
- adicionar rollback para a versão anterior do catálogo;
- alertar quando source estiver usando cache antigo;
- documentar reindexação, rotação de storage e recuperação.

**Aceite:** operador corrige um match sem editar código ou feed; publicação defeituosa pode ser revertida atomicamente.

## Estratégia de migração sem interrupção

1. Criar V3 e importar catálogo canônico em paralelo.
2. Ingerir as mesmas sources nas tabelas V3.
3. Comparar V2 e V3 por plataforma, título, ofertas e cobertura de capas.
4. Ativar `ARCADIA_RETRO_CATALOG_V3=1` somente para desenvolvimento.
5. Fazer shadow reads: servir V2, medir V3 sem mostrá-lo.
6. Liberar V3 para uma porcentagem dos clientes.
7. Manter fallback V2 por pelo menos uma versão do app.
8. Remover V2 somente depois dos critérios de produto serem sustentados.

## Testes necessários

### Unidade

- normalização de URL e fingerprint;
- normalização de SKU/serial;
- aliases, regiões, multi-disc e títulos com cirílico;
- política de confiança e desempate;
- seleção e transformação de artwork;
- sigilo de URI nos DTOs.

### Integração

- importação idempotente do catálogo;
- sync incremental com ETag/304;
- source offline usando snapshot anterior;
- associação de múltiplas sources ao mesmo jogo;
- override preservado após reindexação;
- publicação/rollback atômicos;
- cache/ETag da API e cursor estável.

### Contrato app-servidor

- lista contém somente dados de card;
- detalhe não contém URI;
- resolve retorna URI somente para oferta autorizada;
- app usa fallback após timeout curto;
- versão antiga do app continua funcionando durante a migração.

### Performance

Base de referência: pelo menos 30 mil ofertas e 20 mil jogos.

- busca vazia, busca textual e filtro por plataforma/source;
- 100 clientes simultâneos;
- rebuild completo e sync incremental;
- cold/warm cache de imagens;
- tamanho do espelho local e tempo de startup.

## Observabilidade

Métricas mínimas:

- `retro_catalog_games_total`;
- `retro_offers_total{source}`;
- `retro_match_total{method,confidence_band}`;
- `retro_unmatched_total{reason}`;
- `retro_artwork_coverage{system,provider}`;
- `retro_source_sync_seconds{source,status}`;
- `retro_catalog_search_seconds`;
- `retro_offer_resolve_total{status}`;
- idade da versão ativa e do cache de cada source.

Logs devem carregar `sync_id`, `source_id`, `offer_id`, `game_id` e `matcher_version`, nunca magnet, URI ou credencial.

## Critérios finais de produto

- 100% dos títulos exibidos usam o campo editorial inglês;
- pelo menos 95% dos cards das plataformas com ofertas possuem capa válida;
- pelo menos 95% das ofertas individuais são associadas automaticamente ou revisadas;
- duplicatas visuais por região/release ficam consolidadas em um jogo;
- p95 de busca do servidor abaixo de 100 ms e resposta visual inicial abaixo de 500 ms em rede normal;
- zero consulta de metadata/capa por card no renderer;
- source indisponível não derruba o catálogo ativo;
- URI/magnet não aparece em listagem, detalhe, cache de espelho ou logs;
- toda associação pode ser auditada pelo método, confiança e versão do matcher.

## Ordem recomendada para o Arcadia

A implementação deve começar pelas fases 0–3. A maior falha atual não é visual: é a ausência de uma identidade editorial canônica independente da source. Em seguida, as fases 4–6 entregam a velocidade e as capas percebidas pelo usuário. As fases 7–8 completam a paridade operacional com o Hydra.

O Retro Catalog V2 existente continua útil como base: já possui catálogo versionado, publicação atômica, API paginada, ofertas separadas, ETag, fallback local e resolução de URI sob demanda. O trabalho principal é substituir o título normalizado como autoridade por `provider/objectId + identificadores`, materializar sources no servidor e servir assets próprios otimizados.
