# Catálogo Retro server-first — status de implementação

**Data:** 2026-08-22  
**Status:** implementado; publicação inicial depende do deploy/migration do backend

## Entregue

- Catálogo V2 local com IDs estáveis, jogos consolidados, ofertas separadas, migração V1 e fallback offline.
- Catálogo materializado e versionado no PostgreSQL; uma versão só fica ativa após o rebuild completo.
- Sincronização diária das sources públicas `Classics`, com limite de payload e preservação da versão anterior em caso de falha.
- Índice canônico de capas do Libretro para os 14 sistemas suportados, persistido por sete dias no cache do servidor.
- Títulos editoriais normalizados; entradas cirílicas/CJK sem correspondência inglesa ficam fora da vitrine.
- Collections, BIOS, firmware, updates, DLC e packs `N in 1` ficam fora do catálogo principal.
- Endpoints autenticados para manifest, sistemas, listagem, detalhe, sumário de ofertas e revelação explícita da URI.
- Endpoint autenticado `/catalog/v1/retro/audit` com cobertura por sistema e amostras dos jogos sem capa, detalhes ou hero; exposto no Electron como `retroAudit`.
- Cliente Electron server-first com ETag, espelho local e rollback automático para o V2 local.
- Ofertas pessoais são anexadas localmente no detalhe; seus magnets não entram no espelho nem no backend Retro.
- Cards retrato, contagem separada de jogos/downloads, capas pré-resolvidas e placeholder específico da plataforma.
- A renderização não executa mais uma pesquisa IGDB/SteamGridDB por card.
- A auditoria aceita `system` e `samples` (0–100), usa ETag por consulta e nunca inclui URIs de download.

## Flags e operação

- `ARCADIA_RETRO_SERVER=0`: rollback temporário para o V2 completamente local.
- `ARCADIA_RETRO_V2=0`: rollback adicional para o catálogo legado V1.
- `RETRO_SYNC_ENABLED=0`: desabilita somente o job diário do backend; a versão ativa continua sendo servida.

No primeiro boot do backend após a migration `0004_retro_catalog.sql`, o job começa em cinco segundos. Enquanto a primeira versão é construída, clientes usam o espelho/V2 local.

## Auditoria com o cache real

- Entrada: 27.601 downloads.
- Saída: 22.282 jogos canônicos e 26.755 ofertas.
- Ocultos: 407 títulos não ingleses, 379 releases especiais e 60 itens sem sistema.
- O índice remoto Libretro observado contém 48.224 boxarts nos 14 sistemas.

Esses números são uma auditoria local e podem variar quando as sources mudarem. Capas sem correspondência segura usam o placeholder da plataforma; não é publicada uma URL especulativa que causaria 404 por card.

## Verificação realizada

- App: 278 testes aprovados.
- Renderer: build de produção aprovado.
- Backend: testes puros do Retro e descoberta da migration aprovados.
- A suíte integrada do backend requer `TEST_DATABASE_URL`; sem essa variável ela encerra antes de abrir o banco, conforme a proteção já existente em `server/src/db.js`.
