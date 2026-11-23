# Carregamento das capas Retro

Registro das medições no cliente anterior e no backend compartilhado. Os
trechos sobre React e WebView descrevem a implementação medida na época.

## Cache de navegação no backend

`crates/arcadia-retro/src/catalog.rs` — `RetroServerCatalog` ganhou
`cache: Mutex<HashMap<String, (Instant, Value)>>` com
`CATALOG_CACHE_TTL = 300s`, consultado dentro de `with_fallback` (por onde toda
resposta remota passa).

Medido no log de IPC do app antes da mudança:

```text
store:recent  +2436ms  primeira   |  retro:list  +1133ms  primeira
store:recent     +0ms  segunda    |  retro:list  +1288ms  segunda  ← sem cache
```

A aba Store já tinha cache em memória; a Retro não. Agora refazer uma página
visitada há menos de 5 minutos não custa ida ao servidor. `no_mirror` desliga o
cache junto com o espelho.

Garantia por teste: `catalog::tests::cache_evita_segunda_ida_ao_servidor` conta
as chamadas ao remote e falha se a segunda passagem voltar à rede.

## Capas: 1,25 MB por card

`crates/arcadia-ipc/src/cover_cache.rs` + canal `retro:covers`.

As capas apontam para o tamanho original em `raw.githubusercontent.com` e
`images.launchbox-app.com`, servidas com `cache-control: max-age=300`:

```text
página de 24 jogos ....... 29,4 MB (medido, 8 downloads paralelos)
média por capa ........... 1,25 MB
maior capa (PNG) .........   494 KB
```

O backend baixa cada capa **uma vez**, reduz para 480 px de largura e grava em
disco (`<data_dir>/covers/<sha256>.jpg`, JPEG q82). O card passa a receber o
arquivo local como data URI. Medido numa capa real:

```text
original ......... 494 KB
reduzida .........  77 KB  (15,5% do original)
2a chamada .......  27 µs  (só disco)
```

O canal usa `buffer_unordered(6)`: serial levava **89,4 s** para 24 capas; com 6
em paralelo, **14,1 s** na primeira visita e **0 ms** nas seguintes.

Falha em qualquer etapa devolve a URL remota (comportamento anterior). Cache é
otimização, nunca requisito.

## Rolagem do catálogo

- `ui/src/components/desktop/RetroStoreView.tsx`: o grid do catálogo ganha
  `is-scrolling` durante o gesto (`useScrollActivity`, `ui/src/hooks/`). Com a
  classe ligada, `ui/src/index.css` zera `animation-delay` e
  `animation-duration` dos cards; o estado de repouso é idêntico ao anterior.
  Sem isso, os 24 cards da página mantinham 12 animações `both` com atraso
  escalonado (`nth-child(n+12)` = 220 ms) exatamente no momento em que o
  compositor precisa entregar quadros.
- `contain: layout paint style` no `.retro-catalog-card` passou a valer em
  qualquer resolução. Antes vivia só em `@media (min-width: 1800px)`, então em
  1080p/1440p o compositor reinvalidava a grade inteira por quadro. `size` fica
  de fora de propósito: prender o tamanho colapsaria a capa `aspect-[2/3]`.
- `.arcadia-desktop-retro::after` (véu `position: fixed` cobrindo a viewport,
  `z-index: 90`) foi promovido a camada própria com `transform: translateZ(0)`.
  Sem promoção ele era recomposto junto com o conteúdo a cada quadro.
- O `content-visibility: auto` que já existia é mantido; ele é o que evita
  layout das centenas de cards de páginas anteriores quando o catálogo acumula.

## Mudanças

- `ui/src/components/desktop/retroPageCache.ts`: até 24 páginas recentes,
  separadas por conta, busca, filtro e offset. A mesma requisição em voo é
  compartilhada. Durante 60s, uma página pronta dispensa novo IPC; depois disso,
  a versão conhecida pode aparecer por até 10 minutos enquanto é revalidada.
  Falhas não viram cache permanente.
- A intenção de abrir Retro (hover/foco na aba) e de ir à próxima página já
  antecipa os metadados e até oito capas. São até quatro preloads ativos e
  16 tarefas pendentes. URLs já aquecidas são deduplicadas, a fila tem timeout
  e não retém os bitmaps inteiros após o carregamento.
- `RetroStoreView.tsx`: primeiras oito capas usam `eager`/prioridade alta;
  as demais continuam lazy. A capa e os metadados básicos do card já aparecem
  no detalhe enquanto o IPC completa a ficha. Ações ficam ocupadas até essa
  leitura terminar.
- `../arcadia-server/src/retro-routes.js`: respostas 304 da lista consultam
  somente a versão (uma query, em vez das cinco do fluxo de paginação). O
  detalhe em 304 evita ler ofertas, mas ainda confirma que o jogo existe.
  JSON, ETags, auth e erros de jogo inexistente permanecem preservados.

Não há novo proxy, mudança em Cloudflare, schema ou fontes. As imagens mantêm
URLs e qualidade originais. O cache HTTP do navegador/WebView continua sendo
responsável pelo armazenamento persistente das imagens, respeitando o CDN.

## Evidência e limites

- Amostra pública LaunchBox: 268.910 bytes, cerca de 386ms nessa conexão;
  header `public, max-age=31536000, immutable`. Amostra Libretro: 506.067 bytes,
  cerca de 348ms e `max-age=300`. Não são um benchmark do catálogo inteiro.
- Navegador Orca com IPC simulado e capas públicas reais: hover iniciou a
  consulta e as imagens antes da abertura; voltar à aba reutilizou os 24 cards
  sem outro IPC; as oito imagens prioritárias tinham os atributos esperados.
- Com relógio simulado após o TTL, os 24 cards continuaram visíveis durante
  revalidação. Abrir um detalhe mostrou a capa enquanto uma resposta IPC
  artificialmente atrasada em 1,8s ainda estava pendente, sem skeleton cobrindo
  a ficha e sem erros não tratados.
- Esses testes não são uma promessa de tempo fixo em produção. Uma capa nunca
  baixada, URL nova, cache removido ou conexão lenta ainda dependem da rede.
  O cache de páginas em memória não sobrevive ao encerramento do aplicativo.

## Validação

- `node --test tools/test-retro-cache.mjs`: 7 testes passaram (TTL, isolamento
  por conta, revalidação, limite/timeout da fila, erros e deduplicação).
- `node tools/test-store-search.mjs`: passou.
- `node tools/test-bridge.mjs`: 202 invokes + 22 eventos.
- `node ui/node_modules/typescript/bin/tsc --noEmit -p ui/tsconfig.json`: passou.
- `npm --prefix ui run build`: passou, com aviso de tamanho de chunks existente.
- `cargo build --target-dir target-probe --release -p arcadia --features custom-protocol`: passou.
- No servidor, `npm test`: 70 passaram e 14 integrações foram puladas pela
  ausência de `TEST_DATABASE_URL`. Os três testes novos em
  `test/retro-routes-cache.test.js` usam HTTP local e banco simulado.

Para usar: distribuir/reabrir o novo build desktop e, para a otimização de 304,
publicar o patch do servidor e reiniciar o processo. Nenhum deploy foi feito.
