# Auditoria local — desktop e servidor (2026-11-12)

Registro da auditoria do cliente anterior e do servidor naquela data. Para
compilar e testar o cliente atual, consulte o [README](../README.md).

## Escopo e limites

Repositórios: `arcadia-rust` (`rust-port`) e `../arcadia-server` (`main`).
Revisão orientada por testes, validação de entradas e dados, dependências npm,
fluxos de configuração/loja e inspeção visual com fixtures IPC em navegador.
Não é uma certificação de ausência de bugs nem uma inspeção manual de cada
arquivo. Nada foi publicado, implantado ou executado contra um banco de produção.
Não houve mudança de schema/migration, fontes ou configuração/código do Worker
Cloudflare. Alterações preexistentes em `download_service.rs` foram preservadas.

## Achados corrigidos

| Área / arquivo | Evidência antes da correção | Correção / regressão |
| --- | --- | --- |
| Desktop: `crates/arcadia-ipc/src/lib.rs`, `config:set` | Limpar token com `""` ou `null` deixava o segredo no disco: um segundo merge reintroduzia a chave removida. Teste novo falhou antes da correção. | Persistir o patch sanitizado uma única vez; resposta usa o config efetivamente gravado. Teste cobre remoção de dois segredos e preservação de campos desconhecidos. |
| Desktop: `ui/src/components/desktop/Sidebar.tsx` | Em 960×540, abrir Configurações levava o rodapé para y=687–749, fora da janela; busca/jogos sobrepunham o rodapé. | Navegação e lista passam a ter região rolável; rodapé não encolhe. Após correção, rodapé y=438–500 e conteúdo rolável, sem mudar tamanho mínimo da janela. |
| Desktop: `ui/src/components/desktop/StoreView.tsx` | Rejeição de `storeSearch` deixava botão em “Buscando…” e gerava `Uncaught (in promise)`; sugestões também não tratavam rejeições. | Tratamento de falhas e `finally` preservando a geração da busca atual. Mensagem de erro/resultado vazio deixa de aparecer duas vezes. |
| Servidor: `src/realtime.js` | Mensagem JSON `null` causava TypeError; topic não textual e evento `error` de frame inválido também podiam escapar do callback WebSocket. | Validar envelope e encerrar somente o socket com erro. Teste prova que o join autenticado e a notificação continuam funcionando. |
| Servidor: `src/storage-routes.js`, `limparOrfaos` | Limpeza apagava upload recente ainda sem URL associada ao perfil. Teste em pasta temporária reproduziu perda do arquivo. | Carência de 24h antes de excluir órfãos; arquivos referenciados permanecem. Diretórios simbólicos não são percorridos. |
| Servidor: `src/storage-routes.js`, `mimeFor` | Background/banner PNG era servido como `image/*`. | MIME concreto reaproveita a classificação dos avatares; vídeo mantém seu tipo. Teste HTTP local verifica header e bytes. |
| Servidor: `src/storage-routes.js`, validação | `paths` não-array causava 500; nomes de bucket herdados de Object.prototype não eram rejeitados como bucket inválido. | Retornar 400 para lista inválida e 404 para bucket desconhecido, mantendo a verificação de proprietário. |
| Servidor: `package-lock.json` | `npm audit` apontou cinco alertas (um crítico): body-parser, compression, express, proxy-addr e qs. Não foi feita exploração das vulnerabilidades. | Atualizações dentro das faixas existentes: body-parser 1.20.9, compression 1.8.2, express 4.22.3, proxy-addr 2.0.8, qs 6.17.1. Audit final: zero alertas. |
| Servidor: script/testes | `npm test` falhava no Windows por `NODE_ENV=test`; dois testes pressupunham separador POSIX; teste do Worker confundia crase em comentário com código executável. | Preload de ambiente test independente de shell; caminhos via `node:path`; verificação do Worker ignora somente comentários de linha inteira. O Worker em si não foi alterado. |

## Validação executada

### Desktop

- `cargo test --target-dir target-probe --workspace --exclude arcadia`:
  **1498 passaram, 0 falharam, 1 ignorado** (incluindo doc-tests).
- `cargo test --target-dir target-probe -p arcadia-ipc`: **72 passaram**.
- `cargo build --target-dir target-probe --release -p arcadia --features custom-protocol`: passou.
- `npm --prefix ui run build`: passou; permanece o aviso sobre tamanho de chunks.
- `node ui/node_modules/typescript/bin/tsc --noEmit -p ui/tsconfig.json`: passou.
- `node tools/test-bridge.mjs`: **202 invokes + 22 eventos**.
- `node tools/test-store-search.mjs`: passou; executa os callbacks reais da
  busca e de sugestões, cobrindo falha IPC e isolamento de resposta antiga.
- `npm --prefix ui audit --omit=dev --json`: zero alertas de produção.
- Navegador Orca, build real com IPC simulado e sem perfil real: inspeção de
  Login, Configurações, Loja, Biblioteca e Downloads; busca offline simulada
  passou a mostrar erro e liberar botão sem rejeições não tratadas. Sidebar
  verificada em 960×540 e viewport maior. Biblioteca filtrou o título esperado.

Log completo Rust: `C:/Users/zes/AppData/Local/Temp/opencode/arcadia-audit-rust-tests.log`.

### Servidor

- `npm test`: **56 passaram, 0 falharam, 14 pulados** por ausência de
  `TEST_DATABASE_URL`. Docker/PostgreSQL local não estavam disponíveis.
- Novos testes sem DB real: `test/realtime-validation.test.js`,
  `test/storage-regressions.test.js` e `test/catalog-cache-routes.test.js`.
- Teste do catálogo usa Express/HTTP reais em loopback e Postgres simulado:
  conserva JSON, ordem dos 24 itens, auth/401 e ETag/304; mede duas queries na
  primeira página (catálogo + lote), nenhuma adicional no cache quente, TTL
  e fallback se o Map falha.
- `npm audit --json`: zero alertas após atualizar o lockfile.
- `git diff --check`: passou nos dois repositórios.

## Não comprovado nesta rodada

- Integração real de autenticação, PostgreSQL, sync/realtime entre dispositivos
  e as 14 suítes que dependem de banco. Rodar com banco descartável, nunca com
  `TEST_DATABASE_URL` apontando para produção.
- Aparência/comportamento nativo de todas as telas em WebView2/Linux, jogo real,
  gamepad físico, instalação/desinstalação, debrid real e arquivos grandes.
- A auditoria de dependências feita aqui foi npm; não substitui RustSec nem
  revisão de todos os componentes e caminhos não exercitados.

Os patches estão apenas nos clones locais. Produção precisa de revisão,
deploy e reinício separados; o desktop precisa distribuir o novo build.
