# Carregamento das capas Retro

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
