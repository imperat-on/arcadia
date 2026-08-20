# Arcadia — roteiro de expansão para 500k linhas

## Diagnóstico inicial

A branch `melhorias` parte do commit `022e50b` e contém um launcher Linux funcional
com duas interfaces, sincronização por conta e backend próprio. A leitura inicial
foi feita sobre os módulos Electron, React, indexador Python, backend Node,
scripts, SQL, testes e documentação.

### Mapa atual

- `app/electron/`: processo principal Electron, integrações, indexação, downloads,
  Steam, Wine/Proton, conquistas, trailers, catálogo e sincronização.
- `app/src/`: renderer React/TypeScript com modo desktop e modo console/Big Picture.
- `server/src/`: Express + PostgreSQL + JWT + WebSocket, com autenticação,
  perfis/amigos, sincronização, storage e catálogo.
- `index.py`: descoberta de jogos Steam, Heroic/Epic/GOG e Lutris.
- `ui/`: cliente Godot legado; o caminho principal atual é Electron.
- `docs/`: arquitetura, deploy, schema, auditorias e planos históricos.

O código executável e testes somam aproximadamente 44,5 mil linhas; contando
scripts, SQL e arquivos auxiliares, o repositório fica próximo de 48 mil linhas.
Os maiores pontos de concentração são `app/electron/main.js`,
`app/electron/steamstore.js`, `app/src/components/ps5-launcher/PS5Launcher.tsx`
e `app/src/global.d.ts`.

## Direção técnica

A meta de 500k será alcançada por funcionalidades reais, contratos explícitos,
testes e ferramentas de operação. Cada módulo novo deve ter uma responsabilidade
clara, caminho de execução verificável e teste correspondente.

### Regras de engenharia

1. preservar o funcionamento offline e o isolamento de credenciais;
2. manter o main process como dono de segredos e operações privilegiadas;
3. não duplicar regras entre desktop e console: ambos consumirão os mesmos casos de uso;
4. preferir módulos pequenos, contratos versionados e adaptadores para provedores;
5. medir performance antes/depois (tempo de boot, memória, rede, cache e downloads);
6. toda feature deve incluir testes unitários e, quando aplicável, integração/e2e;
7. dados de usuário devem ter migração, backup e recuperação documentados;
8. código, fixtures e documentação devem refletir APIs reais, não stubs vazios.


### Fase 0.2 — migrations e CI executadas

- `server/src/migrations.js` usa `schema.sql` como baseline v1;
- `0002_performance_indexes.sql` adiciona índices versionados com checksum;
- `db.js` aplica migrations com transação e advisory lock;
- scripts `migrate:postgres`/`verify:postgres` passaram a operar no runner real;
- `docker-compose.test.yml` e `.github/workflows/ci.yml` documentam PostgreSQL 16;
- scripts históricos SQLite foram preservados com nomes explícitos.

### Fase 0.3 — primeiro ganho de boot do frontend

- `PS5Launcher` e `DesktopLauncher` passaram a ser carregados sob demanda;
- o bundle inicial caiu de aproximadamente 1,24 MB para 305 KB;
- o modo não escolhido deixa de ser baixado no primeiro render;
- typecheck e build confirmam os chunks separados.

### Fase 0.4 — contratos compartilhados da biblioteca

- `contracts/index.js` e `contracts/index.d.ts` definem o contrato versionado
  de jogos, biblioteca e payloads de sync;
- Electron normaliza a biblioteca antes de expô-la por IPC;
- backend normaliza `push_library`/playtime antes das queries;
- metadados extras de providers são preservados e entradas inválidas são
  descartadas de forma determinística;
- testes unitários cobrem os consumidores app e server;
- trailer-service.js foi extraído do main process com dependências injetáveis;
- IPC público e payloads do preload foram preservados, incluindo progresso;
- serviço ganhou testes de cache, deduplicação, busca, stream e progresso;
- respostas de status/login do IPC agora removem access/refresh tokens e
  preservam somente a sessão pública documentada.
- `ARCADIA_API_URL` tornou-se a configuração canônica do backend, com aliases
  legados normalizados e testados.

### Fase 0.5 — diagnóstico local seguro

- `diagnostics.js` agrega runtime, armazenamento, biblioteca, fila e snapshots;
- o relatório não inclui caminhos absolutos, credenciais ou payloads de rede;
- IPC/preload tipados expõem `app:diagnostics` para suporte local;
- teste verifica agregação, contagem e ausência de dados sensíveis.

### Fase 0.6 — pacote de suporte local

- `support-bundle.js` exporta diagnóstico e logs limitados em uma pasta escolhida;
- paths locais, tokens, senhas e chaves são redigidos antes da gravação;
- exportação usa diretório temporário e rename atômico;
- novo IPC tipado permite suporte sem enviar dados automaticamente pela rede.

### Fase 1.1 — biblioteca local versionada

- `library.json` passou a usar envelope `version=1` com timestamp, fontes e jogos;
- `library-store.js` lê o formato novo e arrays legados sem migração destrutiva;
- indexador Python, Electron, sync de títulos e watcher de conquistas consomem
  o mesmo leitor;
- escrita continua atômica e versões futuras são rejeitadas com segurança;
- testes cobrem leitura legada, escrita atômica e incompatibilidade de versão.

### Fase 1.2 — execução segura do indexador

- `index-service.js` deduplica execuções concorrentes e expõe timeout/cancelamento;
- `main.js` mantém `runIndexer()` e IPC compatíveis, mas não dispara processos
  duplicados silenciosamente;
- resultado do processo preserva `stdout`, `stderr`, código e diagnóstico;
- testes cobrem sucesso, retry após falha, timeout e cancelamento;
- teste Python do envelope entra no `unittest discover` da CI.

### Fase 1.3 — primeiro provider puro e fixtures

- parser VDF separado em `indexers/parsers.py`, mantendo fallback sem dependência;
- transformação Steam (`build_steam_game`) separada de disco/rede/configuração;
- transformações Legendary, Heroic, Lutris e SLSsteam também são puras;
- `index.py` mantém os entrypoints e a ordem de descoberta existentes;
- fixtures cobrem AppState, ferramentas/runtime, imagens, dedupe e comandos.

### Fase 1.4 — resolução segura de execução

- `launch-resolver.js` separa a política de comando do spawn/Wine/Electron;
- `gameId` ignora comandos arbitrários enviados pelo renderer;
- jogos desconhecidos, comandos vazios e legado fora de `steam://run|install`
  são rejeitados;
- custom games e override de `exePath` continuam usando callbacks do main;
- testes cobrem Steam, legado, custom, override e tentativas de injeção.

### Fase 1.5 — prioridade na fila de downloads

- fila persistida agora normaliza prioridade no intervalo `[-10, 10]`;
- scheduler escolhe maior prioridade e mantém FIFO em empates;
- Epic/Steam aceitam prioridade sem quebrar payloads antigos;
- novo canal `dm:setPriority` mantém controle pelo preload tipado;
- política pura testada sem iniciar Legendary/DepotDownloader.

### Fase 1.6 — logs de execução rotacionados

- `launch-log.js` extrai abertura, rotação e fechamento dos logs de jogos;
- descritores são fechados em sucesso, erro e rejeição de binário;
- política testada sem spawn real e com rotação acima de 5 MiB.

### Fase 1.7 — snapshots locais de saves

- `snapshot-service.js` cria snapshots versionados por jogo, com manifest e cópia
  atômica;
- restauração cria backup automático do destino atual;
- listagem/remoção validam jogo, versão e contenção de paths;
- symlinks são ignorados na origem/restauração e traversal não atravessa a pasta de snapshots;
- restauração faz rollback do destino se a troca atômica falhar;
- IPC/preload expõem a operação sem mover arquivos para a rede.

### Fase 1.8 — integridade e recuperação de downloads

- Legendary só conclui Epic após `legendary verify` retornar sucesso;
- cada depot Steam exige código zero com `-validate` antes de registrar a instalação;
- falhas preservam parciais, usam retry limitado com backoff e não ressuscitam
  após pausa/cancelamento/shutdown;
- estado de integridade e tentativas é persistido e testado por política pura.

### Fase 1.9 — contrato interno de providers

- `ProviderContext` concentra diretório de dados, configuração, idioma e modo
  de rede sem alterar payloads persistidos;
- `ProviderResult` normaliza jogos, warnings, erros e duração da execução;
- `execute_provider` isola exceções e descarta entradas inválidas;
- o pipeline Python usa a fronteira para Steam, Heroic e Lutris;
- testes cobrem sucesso parcial, falha isolada, tipos inválidos e imutabilidade.

### Fase 2.1 — hardening da autenticação

- endpoints públicos de auth recebem limite por IP configurável e headers
  `RateLimit-*`/`Retry-After`;
- rotação de refresh tokens usa transação com lock, impedindo replay concorrente;
- testes cobrem limiter isolado, janela, capacidade e replay (integração DB na CI).

### Fase 2.2 — storage local atômico

- uploads de mídia usam arquivos temporários e `rename` atômico;
- permissões dos novos arquivos são restritas e symlinks locais são rejeitados;
- limpeza de órfãos usa o mesmo `DATA_DIR` do registro das rotas;
- caminhos e magic bytes permanecem validados por testes puros.

## Fases de implementação

### Fase 0 — fundação e segurança de evolução

- contratos compartilhados para jogo, instalação, download, perfil, conquista e catálogo;
- camada de configuração/path resolver única para repo, instalação e dados do usuário;
- comando de diagnóstico e telemetria local (sem dados pessoais);
- pipeline de testes com PostgreSQL efêmero, typecheck e build;
- limites de concorrência, cancelamento e timeouts padronizados;
- extração gradual do `main.js` em serviços de domínio sem alterar IPC público.

### Fase 1 — plataforma de biblioteca e execução

- modelo de biblioteca versionado e migrations locais;
- providers Steam, Epic/Legendary, GOG, Lutris, Wine e jogos custom;
- detecção incremental, indexação paralela e recuperação de arte;
- perfis de execução, logs estruturados, diagnóstico de Proton/Wine;
- snapshots de saves e restauração local;
- filas de instalação/download com retomada, prioridades e integridade.

### Fase 2 — catálogo, descoberta e loja

- catálogo normalizado com cache SWR, ETag, invalidação e busca local;
- recomendações explicáveis, listas, tags, filtros e comparação de requisitos;
- manifests/depot resolver com provedores plugáveis;
- histórico de preços e disponibilidade;
- avaliações, listas públicas e moderação no backend.

### Fase 3 — comunidade e sincronização

- perfil único, amigos, bloqueios, presença e notificações;
- sync de biblioteca, conquistas, horas, saves e preferências por conta;
- conflitos determinísticos, fila offline, auditoria e recuperação;
- reviews, coleções, atividades e privacidade configurável.

### Fase 4 — extensibilidade

- SDK de plugins versionado com permissões declarativas;
- eventos e comandos públicos para integrações;
- adaptadores para emuladores, lojas e serviços externos;
- marketplace/local registry de plugins com assinatura e rollback;
- API local para automação e clientes alternativos.

### Fase 5 — experiência multiplataforma e qualidade

- design system compartilhado entre desktop e console;
- navegação gamepad/teclado/mouse acessível;
- perfis de desempenho, streaming local e controle remoto;
- internacionalização completa e temas;
- observabilidade, benchmarks, fuzzing e testes de recuperação.

## Primeira entrega após este diagnóstico

A próxima implementação deve ser a Fase 0: criar a camada de contratos e o
`path resolver`, adicionar a matriz de validação (testes + typecheck + build) e
começar a extrair serviços de `main.js` mantendo o preload/IPC compatível. Isso
reduz risco para todas as fases seguintes e cria uma base mensurável para a
expansão do projeto.

## Primeira entrega executada

- criado `app/electron/runtime-paths.js` como fonte única de `DATA_DIR`;
- migrados Electron, caches, achievements, indexador Python e preload para
  respeitarem `ARCADIA_DATA_DIR`;
- corrigida a primeira indexação do `arcadia.sh` quando o código está fora da
  pasta de dados;
- `run.sh` passou a encaminhar para o Electron atual, deixando o Godot legado
  fora do caminho principal;
- adicionados testes do resolver e atualizada a documentação de execução;
- unificada a restauração de sessão no IPC, com `SIGNED_IN` para sessões salvas
  válidas e seleção de conta antes do realtime/reconcile;
- adicionados testes do boot de autenticação;
- validação: **76/76 testes do app**, typecheck, build e sintaxe JS/Python/Shell.
