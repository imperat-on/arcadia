# Plano completo: catálogo Retro canônico com ofertas das Sources

**Data:** 2026-08-22  
**Status:** proposta para implementação  
**Escopo:** app Electron, backend de catálogo, contratos, cache, UI da Loja Retro e testes  
**Objetivo:** transformar a Loja Retro de uma listagem de downloads Hydra em um catálogo de jogos único, com capas e metadados confiáveis, mantendo as Sources configuradas como ofertas de download.

## Resumo executivo

O Libretro cobre, como **base de identificação**, todos os sistemas explicitamente suportados pelos emuladores nativos do Arcadia:

- PlayStation 1 (DuckStation);
- PlayStation 2 (PCSX2);
- PlayStation 3 (RPCS3);
- GameCube e Wii (Dolphin);
- PlayStation Portable (PPSSPP);
- Nintendo DS/DSi (melonDS e DeSmuME);
- os sistemas clássicos aceitos pelo perfil multi-sistema do RetroArch, incluindo NES, SNES, GB, GBC, GBA e Nintendo 64.

Isso **não significa cobertura uniforme de capas nem existência de um core Libretro para todos eles**. A própria documentação do `libretro-database` marca PS3 como uma base preventiva, sem emulador Libretro. A cobertura atual de box art também é baixa em PS3 e parcial em Wii, PSP e GameCube. Portanto:

> O Libretro deve ser a primeira camada de identidade e a primeira tentativa de arte, mas não pode ser o único provedor do Arcadia.

A solução proposta usa:

1. **Libretro Database** para nomes canônicos, sistemas, serial/checksum e aliases;
2. **Libretro Thumbnails** como capa preferencial sem chave;
3. **IGDB** para completar capa, descrição, data, gênero, desenvolvedor e publisher;
4. **SteamGridDB** como arte opcional de alta qualidade quando houver chave do usuário;
5. **Sources Hydra** somente como ofertas de download anexadas ao jogo canônico;
6. **cache local e servidor** para evitar chamadas em lote a serviços de terceiros.

## Diagnóstico do estado atual

### Código

O arquivo `app/electron/retro-catalog.js` atualmente:

1. consulta `https://api.hydralibrary.com/sources?status=Classics&limit=100`;
2. baixa cada JSON de source marcada como `Classics`;
3. transforma cada entrada de `downloads[]` em um objeto de jogo;
4. gera o ID como `sourceId:index`;
5. pagina esses downloads diretamente na Loja Retro.

Consequências:

- um download equivale incorretamente a um jogo;
- versões de região, idioma, tradução e formato aparecem duplicadas;
- packs, coletâneas, hacks, patches e homebrew disputam espaço com jogos individuais;
- o ID muda se a source reordenar o array;
- metadados e capa dependem de um título de release ruidoso;
- uma mesma obra em duas sources não é consolidada;
- a UI chama o total de downloads de “jogos”.

O renderer tenta reparar a falta de arte em `app/src/components/desktop/retroArtwork.ts`, consultando a bridge de metadados para cada card sem capa. A fila limita concorrência, mas continua sendo enriquecimento tardio, por título, durante a navegação.

### Dados reais observados nesta máquina

O cache `~/.local/share/arcadia/retro-catalog.json` contém 27.601 entradas provenientes de quatro feeds:

| Source | Entradas declaradas |
|---|---:|
| PsxRoms | 334 |
| PS1-PS3 Rutracker | 7.156 |
| RetroArch Games | 20.051 |
| cdogbruv custom library | 60 |

Distribuição efetiva do campo `platform`:

| Plataforma recebida | Downloads |
|---|---:|
| `nes` | 6.494 |
| `snes` | 4.123 |
| `ps2` | 3.838 |
| `gba` | 3.559 |
| `gbc` | 2.471 |
| `ps1` | 2.210 |
| `gb` | 2.184 |
| `ps3` | 1.442 |
| `n64` | 1.220 |
| vazio | 60 |

No cache atual ainda não há ofertas marcadas como GameCube, Wii, PSP ou DS, embora os emuladores correspondentes existam no registry. O desenho deve suportá-las antes que uma source nova as forneça.

## Compatibilidade Libretro × emuladores do Arcadia

### Matriz de decisão

Os percentuais abaixo são apenas um indicador de cobertura do conjunto `Boxarts + Snaps + Titles`, não a porcentagem direta de jogos com capa. A coluna “box arts” é mais útil para a Loja.

| Emulador Arcadia | Sistema | Base Libretro | Repositório de thumbnails | Box arts observadas | Estratégia |
|---|---|---|---|---:|---|
| DuckStation | Sony PlayStation | sim, Redump/TOSEC | sim | 8.898 / 10.758 | Libretro primeiro; IGDB fallback |
| PCSX2 | Sony PlayStation 2 | sim, Redump | sim | 8.366 / 11.346 | Libretro primeiro; IGDB fallback |
| RPCS3 | Sony PlayStation 3 e PSN | sim, GameTDB/Redump/No-Intro | sim, físico e downloadable | 43 / 18.605 no conjunto físico medido | serial/Title ID primeiro; IGDB/SGDB essenciais |
| Dolphin | Nintendo GameCube | sim, Redump/GameTDB | sim | 1.928 / 4.847 | Libretro + GameTDB; IGDB fallback |
| Dolphin | Nintendo Wii | sim, GameTDB/Redump | sim | 4.384 / 18.981 | GameTDB/Libretro + IGDB fallback |
| PPSSPP | Sony PSP e PSP PSN | sim, No-Intro/Redump | sim | 1.667 / 5.495 | Libretro primeiro; IGDB fallback |
| melonDS | Nintendo DS/DSi | sim, No-Intro | sim | DS: 6.673 / 8.063; DSi: 10 / 1.847 | DS Libretro primeiro; DSi exige fallback |
| DeSmuME | Nintendo DS/DSi | sim, No-Intro | sim | igual ao melonDS | mesma identidade, outro runtime |
| RetroArch | NES | sim, No-Intro | sim | 2.929 / 5.310 | Libretro primeiro |
| RetroArch | SNES | sim, No-Intro | sim | 3.531 / 3.782 | Libretro primeiro |
| RetroArch | Game Boy | sim, No-Intro | sim | 1.620 / 1.903 | Libretro primeiro |
| RetroArch | Game Boy Color | sim, No-Intro | sim | 1.404 / 1.900 | Libretro primeiro |
| RetroArch | Game Boy Advance | sim, No-Intro | sim | 3.167 / 3.357 | Libretro primeiro |
| RetroArch | Nintendo 64 | sim, No-Intro | sim | 1.033 / 1.140 | Libretro primeiro |

### Conclusão da compatibilidade

- **Sim para catálogo/identidade:** todos os sistemas explícitos atuais têm database Libretro.
- **Não para runtime:** PS3 não tem core Libretro; isso não interfere no uso da base pelo RPCS3.
- **Não como fonte única de capas:** PS3, Wii, PSP, GameCube e DSi apresentam lacunas importantes.
- **Emulador e sistema devem ser entidades separadas:** melonDS e DeSmuME compartilham `nintendo-ds`; Dolphin compartilha GameCube/Wii; RetroArch pode executar vários sistemas.

## Fontes de dados e prioridade

### 1. Libretro Database — identidade

Uso:

- nome canônico;
- nome da database/sistema;
- serial para mídia óptica quando disponível;
- CRC/SHA-1/MD5 para cartuchos e arquivos menores;
- região, idioma, ano, gênero e publisher quando presentes;
- aliases derivados de nomes de releases.

Não usar para:

- decidir qual emulador standalone será executado;
- presumir que todo registro tenha descrição completa;
- presumir que todo nome tenha uma imagem correspondente.

### 2. Libretro Thumbnails — arte preferencial

Prioridade de tentativa:

1. `Named_Boxarts`;
2. `Named_Titles` como fallback visual;
3. `Named_Snaps` somente como último fallback.

Não clonar o metarrepositório completo no computador do usuário. O Arcadia deve manter um índice compacto no backend e baixar imagens sob demanda, com cache local.

### 3. IGDB — enriquecimento

Uso:

- capa quando Libretro não possui box art;
- resumo, storyline, data de lançamento;
- gêneros, desenvolvedor, publisher;
- artworks e screenshots para a página de detalhe.

Regras:

- nunca consultar o proxy público do Playnite para dezenas de milhares de jogos em lote;
- resolver somente jogos visíveis, pesquisados ou com oferta recém-importada;
- persistir resultado e resultado negativo com TTL;
- futuramente preferir credenciais próprias do backend se os termos e a operação permitirem;
- usar plataforma e ano no ranking; nunca aceitar apenas o primeiro resultado fuzzy.

### 4. SteamGridDB — arte opcional

Uso:

- capa de maior resolução;
- hero e logo;
- escolha manual do usuário.

Como a chave é local, o servidor não deve receber esse segredo. A seleção automática pode ocorrer no app e ser persistida como override local.

### 5. Feed Hydra — disponibilidade

O feed informa somente:

- que há uma oferta;
- source de origem;
- URI/magnet;
- tamanho, data, descrição de release e título original;
- plataforma declarada quando houver.

O feed não é a autoridade de identidade, capa ou descrição editorial.

## Arquitetura proposta

```text
Hydra registry + sources configuradas
                 |
                 v
        normalizador de ofertas
                 |
       +---------+----------+
       |                    |
 plataforma/serial     título normalizado
       |                    |
       +---------+----------+
                 v
          resolvedor canônico
      Libretro -> alias -> IGDB
                 |
       +---------+----------+
       |                    |
  jogo canônico         oferta/source
       |                    |
       +---------+----------+
                 v
       índice paginado da Loja Retro
                 |
                 v
       detalhe + seletor de downloads
```

### Separação de responsabilidades

#### Backend

- sincronizar manifestos/índices públicos permitidos;
- gerar índice canônico compacto por sistema;
- expor busca e detalhe paginados;
- manter correspondências automáticas reutilizáveis;
- não armazenar URI privada nem API key do usuário;
- opcionalmente enriquecer metadados públicos conforme credenciais próprias.

#### Main process Electron

- ler as sources configuradas para a conta;
- validar e normalizar ofertas;
- relacionar ofertas com o índice canônico;
- manter URIs somente no escopo local;
- mesclar cache remoto, espelho e overrides locais;
- iniciar downloads sem expor todas as URIs na listagem.

#### Renderer

- listar jogos, não releases;
- exibir número de ofertas;
- filtrar por sistema, região, idioma e source;
- abrir detalhe e escolher uma oferta;
- permitir correção manual de capa/matching sem editar o feed.

## Modelo de dados

### Sistema

```ts
interface RetroSystem {
  id: string                 // sony-playstation-2
  displayName: string        // PlayStation 2
  libretroDatabase: string   // Sony - PlayStation 2
  thumbnailCollection: string
  emulatorIds: string[]      // ["pcsx2"]
  aliases: string[]          // ["ps2", "playstation 2", "sony ps2"]
  mediaType: "cartridge" | "disc" | "digital" | "mixed"
  identityStrategy: ("serial" | "sha1" | "crc" | "title")[]
}
```

### Jogo canônico

```ts
interface RetroCatalogGame {
  id: string                 // retro:sony-playstation-2:SLUS-20312
  systemId: string
  title: string
  sortTitle: string
  aliases: string[]
  serials: string[]
  hashes?: { crc32?: string[]; md5?: string[]; sha1?: string[] }
  regions: string[]
  languages: string[]
  releaseDate?: string
  developer?: string
  publisher?: string
  genres?: string[]
  summary?: string
  artwork: {
    cover?: string
    titleScreen?: string
    screenshot?: string
    hero?: string
    logo?: string
    provider?: "libretro" | "igdb" | "steamgriddb" | "source" | "manual"
  }
  offerCount: number
  matchQuality?: "exact" | "strong" | "probable" | "unmatched"
}
```

### Oferta local

```ts
interface RetroOffer {
  id: string                 // hash estável de source + identidade do release
  gameId?: string            // ausente enquanto unmatched
  sourceId: string
  sourceTitle: string
  originalTitle: string
  normalizedTitle: string
  systemId?: string
  platformRaw?: string
  serials?: string[]
  region?: string
  languages?: string[]
  releaseKind: "game" | "collection" | "hack" | "translation" | "homebrew" | "dlc" | "update" | "bios" | "unknown"
  fileSize?: string
  uploadDate?: string
  description?: string
  uris: string[]
  match: {
    method: "serial" | "hash" | "exact-title" | "alias" | "fuzzy" | "manual" | "none"
    confidence: number
    catalogVersion: number
  }
}
```

### Overrides

```ts
interface RetroMatchOverride {
  offerFingerprint: string
  gameId: string | null       // null = ignorar
  artworkUrl?: string
  updatedAt: number
}
```

Overrides permanecem locais por conta e têm precedência sobre rematching automático.

## Identidade estável

### IDs de jogos

Ordem:

1. `retro:<system>:<serial-normalizado>` para sistemas ópticos;
2. `retro:<system>:sha1:<hash>` para ROMs conhecidas;
3. `retro:<system>:crc32:<hash>` quando essa for a chave da database;
4. `retro:<system>:slug:<hash-curto-do-título-canônico>` apenas como fallback.

Não usar índice do array, posição da página ou URL de capa.

### IDs de ofertas

```text
sha256(sourceId + "\0" + originalTitle + "\0" + uris-normalizadas).slice(0, 24)
```

Se a ordem do feed mudar, a oferta mantém o ID.

## Registro de sistemas

Criar `app/electron/retro-systems.js` como fonte única de aliases:

| Entrada da source | `systemId` | Database/thumbnail collection |
|---|---|---|
| `ps1`, `psx`, `playstation`, `playstation 1` | `sony-playstation` | `Sony - PlayStation` |
| `ps2`, `playstation 2` | `sony-playstation-2` | `Sony - PlayStation 2` |
| `ps3`, `playstation 3` | `sony-playstation-3` | físico ou downloadable conforme release |
| `psp`, `playstation portable` | `sony-psp` | `Sony - PlayStation Portable` |
| `gc`, `gcn`, `gamecube` | `nintendo-gamecube` | `Nintendo - GameCube` |
| `wii` | `nintendo-wii` | `Nintendo - Wii` |
| `nds`, `ds`, `nintendo ds` | `nintendo-ds` | `Nintendo - Nintendo DS` |
| `dsi`, `nintendo dsi` | `nintendo-dsi` | `Nintendo - Nintendo DSi` |
| `nes`, `famicom` | `nintendo-nes` | `Nintendo - Nintendo Entertainment System` |
| `snes`, `sfc`, `super famicom` | `nintendo-snes` | `Nintendo - Super Nintendo Entertainment System` |
| `gb`, `game boy` | `nintendo-game-boy` | `Nintendo - Game Boy` |
| `gbc`, `game boy color` | `nintendo-game-boy-color` | `Nintendo - Game Boy Color` |
| `gba`, `game boy advance` | `nintendo-game-boy-advance` | `Nintendo - Game Boy Advance` |
| `n64`, `nintendo 64` | `nintendo-64` | `Nintendo - Nintendo 64` |

O registry deve aceitar aliases adicionais por plugin, mas impedir que um plugin substitua os IDs built-in.

## Pipeline de matching

### Etapa 1 — normalizar sem perder proveniência

Preservar sempre `originalTitle`. Produzir campos separados:

- `normalizedTitle`;
- `edition`;
- `region`;
- `languages`;
- `format`;
- `serials`;
- `releaseKind`;
- `systemId`.

O parser atual de `retro-catalog.js` deve ser extraído para `retro-title-parser.js` e continuar puro/testável.

### Etapa 2 — classificar releases

- jogos individuais entram no catálogo principal;
- collections/packs podem gerar uma coleção navegável, mas não um falso jogo;
- BIOS deve ser ocultada e nunca oferecida como jogo;
- DLC/update exige jogo pai identificado;
- hacks, traduções e homebrew usam badge próprio;
- entrada sem plataforma vai para fila `unmatched`, não para associação global por título.

### Etapa 3 — resolver por evidência

Pontuação sugerida:

| Evidência | Pontos |
|---|---:|
| serial exato no mesmo sistema | 100 |
| SHA-1/MD5/CRC exato | 100 |
| título canônico exato + sistema | 85 |
| alias exato + sistema | 80 |
| título normalizado forte + sistema + região | 75 |
| fuzzy + sistema + ano/região | até 69 |
| somente título, plataforma ausente | máximo 45 |

Política:

- `>= 80`: associar automaticamente;
- `65–79`: associar como `probable`, sujeito a auditoria e sem mesclar edições conflitantes;
- `< 65`: manter unmatched;
- nunca cruzar sistemas apenas porque os títulos são iguais;
- overrides manuais vencem qualquer pontuação.

### Etapa 4 — consolidar ofertas

Um jogo canônico pode conter várias ofertas. Não concatenar todas as URIs cegamente: cada botão precisa preservar source, tamanho, idioma, região e release original.

### Etapa 5 — resolver artwork

```text
override manual
  -> capa válida do feed associada com confiança
  -> Libretro Named_Boxarts
  -> IGDB com plataforma/ano
  -> SteamGridDB local, se configurado
  -> Named_Titles
  -> Named_Snaps
  -> placeholder por sistema
```

Uma resposta negativa deve ser cacheada por 24 horas para impedir tempestade de requisições.

## Backend e endpoints

### Novas tabelas ou namespaces de `catalog_cache`

Primeiro corte pode reutilizar `catalog_cache`:

- `retro:manifest:v1`;
- `retro:system:<systemId>:v1`;
- `retro:game:<gameId>:v1`;
- `retro:search:<normalizedQuery>:<systemId>:v1`;
- `retro:art:<gameId>:v1`.

Se volume/consulta exigir, migrar depois para tabelas normalizadas. Não começar com banco relacional complexo sem medir o índice compacto.

### Rotas propostas

```text
GET /catalog/v1/retro/manifest
GET /catalog/v1/retro/games?system=&query=&offset=&limit=
GET /catalog/v1/retro/games/:id
GET /catalog/v1/retro/systems
GET /catalog/v1/retro/art/:id
```

As rotas públicas nunca devolvem URI Hydra. O app mescla ofertas locais após receber a página canônica.

### Atualização do índice

- job controlado no backend, não na inicialização de cada cliente;
- manifest com `version`, `generatedAt`, `sourceCommits` e checksum;
- escrita atômica;
- manter a versão anterior se a geração falhar;
- stale-while-revalidate alinhado ao catálogo atual;
- permitir fixtures pequenas no repositório para testes, sem versionar a base inteira.

## Cache local

Arquivos propostos:

```text
ARCADIA_DATA_DIR/
  retro/
    catalog-manifest.json
    catalog-index-v2.json
    offers-v2.json
    matches-v2.json
    artwork-v1.json
    unmatched-v1.json
    overrides-v1.json             # por conta
```

Regras:

- todos os arquivos com envelope versionado;
- limite de tamanho e quantidade;
- rejeição de symlink como no código atual;
- escrita `tmp + rename`;
- URIs só em `offers-v2.json`, nunca no índice público;
- cache de artwork guarda URL, provider, resolução e timestamps;
- migração idempotente do `retro-catalog.json` v1;
- cache antigo não é apagado até o v2 ser validado com sucesso.

## Contratos IPC

### Listagem

```ts
retroList({ query, systems, sources, offset, limit, refresh }): Promise<{
  ok: boolean
  games: RetroCatalogGameSummary[]
  totalGames: number
  totalOffers: number
  unmatchedOffers: number
  facets: {
    systems: Record<string, number>
    sources: Record<string, number>
    releaseKinds: Record<string, number>
  }
  offset: number
  limit: number
  hasMore: boolean
  updatedAt?: number
  error?: string
}>
```

### Detalhe

```ts
retroGame(gameId): Promise<{
  ok: boolean
  game?: RetroCatalogGame
  offers?: RetroOfferSummary[]       // sem URI ainda
  error?: string
}>
```

### Oferta selecionada

```ts
retroOffer(offerId): Promise<{
  ok: boolean
  offer?: RetroOffer                 // URI somente após ação explícita
  error?: string
}>
```

### Correções locais

```ts
retroSetMatch({ offerId, gameId }): Promise<Result>
retroIgnoreOffer(offerId): Promise<Result>
retroSetArtwork({ gameId, url }): Promise<Result>
```

## UI/UX

### Grade

- card representa jogo canônico;
- capa retrato em `aspect-[2/3]`, não `460/215`;
- título canônico e nome do sistema;
- badge “N fontes” ou “N downloads”;
- badge de hack/homebrew apenas quando o jogo inteiro for dessa categoria;
- skeleton enquanto a capa é resolvida, sem layout shift.

### Busca e filtros

- busca por título e alias;
- filtros por sistema, source, região, idioma e tipo de release;
- busca não deve depender da rede depois que o índice estiver sincronizado;
- ordenação estável por `sortTitle + systemId + id`.

### Detalhe

- hero/cover e metadados do jogo;
- seletor de ofertas com source, versão, região, idioma, tamanho e data;
- URI revelada apenas depois de selecionar a oferta;
- botão separado para abrir o registro da source Hydra;
- indicação de baixa confiança quando o matching for provável;
- ação “Corrigir correspondência” para entradas problemáticas.

### Contadores

Substituir “27.601 jogos” por valores semanticamente corretos:

```text
18.420 jogos · 27.601 downloads disponíveis
```

Os números acima são apenas exemplo; o total canônico real será obtido após matching.

## Segurança e privacidade

- manter `safePublicUrl`, limites de payload e bloqueio de rede privada;
- renderer nunca busca JSON remoto diretamente;
- listagem nunca recebe magnets/URIs;
- URI só atravessa IPC após escolha explícita;
- não enviar sources privadas, API keys ou histórico ao backend;
- não executar texto de source como HTML;
- normalizar caminhos de thumbnail sem permitir `..`, barra ou controle;
- validar MIME e tamanho de imagem antes de persistir em cache;
- manter downloads e conteúdo de BIOS fora do catálogo público;
- documentar que o Arcadia agrega metadados e links configurados pelo usuário, sem hospedar conteúdo dos jogos.

## Observabilidade

Registrar métricas locais sem conteúdo sensível:

- total de ofertas importadas;
- jogos canônicos criados;
- duplicatas consolidadas;
- match por método/confiança;
- ofertas unmatched;
- cobertura de capa por provider/sistema;
- latência e hit rate de cache;
- erros por source sem registrar URI completa.

Adicionar uma tela/ação de diagnóstico exportável com:

- versões dos índices;
- data da última atualização;
- contagem por sistema;
- sources falhando;
- amostra sanitizada de motivos de unmatched.

## Migração compatível

1. Ler `retro-catalog.json` v1 somente como entrada de migração.
2. Gerar fingerprint estável de cada download.
3. Resolver sistema e título com o parser novo.
4. Associar ao índice canônico ou marcar unmatched.
5. Gravar todos os arquivos v2 atomicamente.
6. Validar contagens e referências.
7. Ativar v2 por feature flag.
8. Manter fallback para v1 por uma versão do app.
9. Remover fallback somente após telemetria/diagnóstico local confirmar estabilidade.

Feature flags sugeridas:

```text
retroCatalogV2
retroLibretroArtwork
retroMetadataEnrichment
retroManualMatching
```

## Plano de implementação

### Fase 0 — fixtures e medição

**Objetivo:** congelar o comportamento real antes da mudança.

- [ ] Criar fixtures sanitizadas das quatro sources atuais, incluindo PS1, PS2, PS3, NES/SNES/GB/GBC/GBA/N64, packs, patches e títulos sem plataforma.
- [ ] Criar script read-only de auditoria que produza contagens por plataforma, release kind e duplicata provável.
- [ ] Registrar baseline de tempo, memória, tamanho do cache e cobertura de capa.
- [ ] Definir 100 casos dourados de matching manualmente revisados.

**Arquivos:**

- `app/fixtures/retro/`
- `app/scripts/audit-retro-catalog.js`
- `app/test/retro-audit.test.js`

**Aceite:** relatório determinístico e nenhuma URI real nos fixtures versionados.

### Fase 1 — registro de sistemas e parser

**Objetivo:** separar plataforma, título e classificação de release.

- [ ] Criar `retro-systems.js`.
- [ ] Extrair parser de `retro-catalog.js` para `retro-title-parser.js`.
- [ ] Adicionar extração de serial/Title ID para textos de PS1/PS2/PS3/PSP.
- [ ] Classificar collection/hack/translation/homebrew/DLC/update/BIOS.
- [ ] Preservar título original e campos extraídos.

**Testes:** aliases, falsos positivos, Unicode, títulos truncados, `.hack`, região, idioma, packs e entradas maliciosas.

**Aceite:** 100% dos casos dourados preservam provenance e pelo menos 95% resolvem o sistema correto quando a source contém plataforma.

### Fase 2 — índice canônico Libretro

**Objetivo:** disponibilizar identidade sem carregar bases enormes no renderer.

- [ ] Criar importador offline no backend para dados Libretro permitidos.
- [ ] Gerar índice compacto por sistema com IDs, nomes, aliases, serials e hashes.
- [ ] Publicar manifest/checksums e endpoints paginados.
- [ ] Persistir source commit/licença/proveniência.
- [ ] Adicionar fixtures mínimas para todos os sistemas do registry.

**Arquivos previstos:**

- `server/src/retro/catalog-builder.js`
- `server/src/retro/libretro-parser.js`
- `server/src/retro/routes.js`
- `server/test/retro-catalog.test.js`
- `app/electron/retro-canonical-client.js`

**Aceite:** todos os sistemas built-in têm ao menos uma fixture resolvível; atualização falha sem destruir a versão anterior.

### Fase 3 — offers e matching local

**Objetivo:** transformar 27.601 downloads em ofertas associadas a jogos.

- [ ] Criar repositório versionado de ofertas.
- [ ] Implementar fingerprint estável.
- [ ] Implementar ranking por serial/hash/título/alias/plataforma.
- [ ] Guardar explicação e confiança do match.
- [ ] Manter unmatched separado.
- [ ] Consolidar ofertas sem perder atributos de release.

**Arquivos previstos:**

- `app/electron/retro-offers.js`
- `app/electron/retro-matcher.js`
- `app/electron/retro-repository.js`
- `app/test/retro-matcher.test.js`
- `app/test/retro-repository.test.js`

**Aceite:** nenhum jogo é unido entre sistemas; IDs sobrevivem a reordenação de feed; nenhuma URI entra no índice público.

### Fase 4 — pipeline de artwork

**Objetivo:** capa confiável, cacheada e sem tempestade de requests.

- [ ] Criar resolvedor de URL Libretro com encoding seguro.
- [ ] Validar a existência da capa com cache negativo.
- [ ] Melhorar matching IGDB para considerar plataforma e ano.
- [ ] Manter SteamGridDB como fallback local/opção manual.
- [ ] Persistir provider e resolução escolhidos.
- [ ] Migrar a busca tardia do renderer para serviço no main/backend.

**Arquivos previstos:**

- `app/electron/retro-artwork.js`
- `app/test/retro-artwork-service.test.js`
- ajustes em `app/electron/metadata.js`
- redução de responsabilidade em `app/src/components/desktop/retroArtwork.ts`

**Aceite:** abrir uma página já cacheada não faz chamadas externas; uma falha não é repetida por card; fallback respeita sistema.

### Fase 5 — IPC e UI v2

**Objetivo:** experiência semelhante a uma loja, com downloads como opções.

- [ ] Atualizar contratos TypeScript e preload.
- [ ] Implementar `retroOffer` para revelar URI sob demanda.
- [ ] Refazer grade em proporção de box art.
- [ ] Adicionar facets, contagens de jogos/ofertas e badges.
- [ ] Criar seletor de ofertas no detalhe.
- [ ] Tratar coleção/unmatched separadamente.
- [ ] Atualizar traduções pt-BR, en-US e es-ES.

**Arquivos previstos:**

- `app/electron/main.js`
- `app/electron/preload.js`
- `app/src/global.d.ts`
- `app/src/components/desktop/RetroStoreView.tsx`
- `app/src/components/desktop/retroArtwork.ts`
- `app/src/i18n/*.json`

**Aceite:** um jogo com três releases aparece em um card e três opções no detalhe; a listagem não contém URI.

### Fase 6 — overrides e recuperação

**Objetivo:** corrigir os casos que nenhuma API resolve bem.

- [ ] Implementar override de matching por conta.
- [ ] Permitir ignorar falso jogo/BIOS/pack.
- [ ] Permitir capa manual usando validação já existente.
- [ ] Expor relatório de unmatched.
- [ ] Garantir que atualização do catálogo não apague overrides.

**Aceite:** override continua válido após refresh e troca de ordem das sources.

### Fase 7 — migração, benchmark e rollout

**Objetivo:** ativar v2 sem regressão ou perda de catálogo offline.

- [ ] Implementar migrador v1 → v2 idempotente.
- [ ] Testar cache corrompido, offline, 403 e source parcialmente indisponível.
- [ ] Benchmark com as 27.601 entradas reais.
- [ ] Ativar inicialmente por feature flag.
- [ ] Comparar contagem, matching e cobertura por sistema.
- [ ] Documentar rollback para v1.

**Metas iniciais:**

- listagem cacheada em até 200 ms no hardware de desenvolvimento;
- pesquisa local em até 100 ms;
- memória adicional inferior a 150 MB durante rebuild;
- zero URI no payload de `retroList` e `retroGame`;
- 100% de IDs estáveis após reordenação do mesmo feed;
- pelo menos 95% dos jogos individuais das sources atuais associados automaticamente;
- zero associação automática entre plataformas diferentes;
- cobertura de capa medida separadamente por sistema, sem uma meta global enganosa.

## Estratégia de testes

### Unitários

- normalização de plataformas;
- parsing de título, região, idioma, serial e release kind;
- geração de IDs/fingerprints;
- ranking e limiares de confiança;
- URL/filename seguro para thumbnails;
- merge de artwork e overrides;
- serialização versionada.

### Contrato

- parser do índice Libretro;
- endpoints do backend;
- envelopes IPC;
- ausência de URI em list/detail;
- URI presente somente em `retroOffer`;
- compatibilidade do cache v1.

### Integração

- source → oferta → jogo → detalhe → download;
- duas sources para o mesmo jogo;
- mesmo título em plataformas diferentes;
- PS3 físico × PSN;
- DS × DSi;
- collection que não deve virar jogo;
- source offline com cache anterior;
- metadata provider offline;
- troca de conta e overrides isolados.

### UI

- grade retrato;
- paginação e facets;
- card único com várias ofertas;
- navegação por teclado/gamepad;
- loading/error/empty/unmatched;
- imagens quebradas e fallback;
- traduções.

### Segurança

- SSRF e URL privada;
- path traversal em nome de thumbnail;
- payload grande;
- symlink em cache;
- HTML/script em descrição;
- magnet inválido;
- source maliciosa tentando substituir IDs built-in;
- renderer tentando solicitar offerId inexistente.

## Riscos e mitigação

| Risco | Impacto | Mitigação |
|---|---|---|
| nomes Libretro e thumbnails divergirem | capa ausente | índice de aliases, tentativa flexível e fallback IGDB |
| cobertura PS3 muito baixa | muitos placeholders | serial/Title ID + IGDB/SGDB; não depender de Libretro artwork |
| título de source não identificar plataforma | match errado | não autoassociar abaixo do limiar; fila unmatched |
| proxy público IGDB ficar indisponível | enriquecimento falha | cache, timeout, circuit breaker e Libretro/source fallback |
| índice completo ficar grande | startup/memória | shard por sistema, manifest e busca no backend/índice compacto |
| atualização upstream alterar nomes | IDs instáveis | IDs por serial/hash; alias do nome anterior |
| pack conter dezenas de jogos | card enganoso | classificar como collection e processar manifesto apenas futuramente |
| plugin adicionar plataforma conflitante | identidade corrompida | namespace do plugin e built-ins imutáveis |

## Fora do primeiro rollout

- extrair automaticamente todos os jogos internos de packs sem manifesto;
- baixar ou distribuir BIOS/firmware;
- calcular hash integral de ISOs remotas;
- substituir os emuladores standalone por cores Libretro;
- sincronizar API keys ou magnets com o servidor;
- importar a totalidade das imagens Libretro para o repositório Arcadia;
- matching por visão computacional de capas.

## Ordem recomendada de entrega

1. sistema/parser e auditoria;
2. índice canônico mínimo apenas para as plataformas presentes hoje;
3. matching e consolidação local;
4. UI com jogo único + ofertas;
5. artwork Libretro e cache;
6. IGDB/SteamGridDB fallback;
7. ampliar fixtures para GameCube, Wii, PSP e DS;
8. overrides, diagnóstico e rollout.

Essa ordem entrega rapidamente a correção mais visível — remover duplicatas e packs da grade principal — sem bloquear tudo na obtenção perfeita de capas.

## Referências verificadas

- Libretro Database: https://github.com/libretro/libretro-database
- Libretro Thumbnails: https://github.com/libretro-thumbnails/libretro-thumbnails
- Cobertura de thumbnails comparada à database: https://github.com/RobLoach/libretro-thumbnails-check
- Guia de playlists e thumbnails do RetroArch: https://docs.libretro.com/guides/roms-playlists-thumbnails/
- Documentação da API IGDB: https://api-docs.igdb.com/
- Registro local de emuladores do Arcadia: `app/electron/emulator-registry.js`
- Catálogo Retro atual: `app/electron/retro-catalog.js`
- Pipeline de metadados atual: `app/electron/metadata.js`
- Renderer atual: `app/src/components/desktop/RetroStoreView.tsx`

## Decisão final

Adotar **catálogo híbrido**:

- Libretro define a espinha dorsal dos sistemas e da identidade;
- IGDB e SteamGridDB completam arte/metadados onde necessário;
- Hydra informa disponibilidade e downloads;
- Arcadia mantém matching, cache, overrides e apresentação.

Não adotar “Libretro como API única”, porque isso funcionaria bem para várias plataformas clássicas, mas produziria uma Loja Retro incompleta especialmente para PS3, Wii, PSP, GameCube e DSi.
