# Sync offline e resolução de conflitos

O sync do Arcadia continua sendo executado no **main process**. O contrato
público de IPC (`sync:now`, `sync:state` e os eventos `sync:state`) não mudou.

## Regra pura

`contracts/sync.js` contém somente funções determinísticas. Elas não acessam
Electron, rede, relógio ou arquivos; por isso podem ser usadas tanto no main
quanto em testes e não misturam dados entre contas:

- **conquistas:** o menor `unlocked_at` válido vence (earliest-wins);
- **biblioteca:** a maior `revision`/timestamp vence; se o backend antigo não
  trouxer versão, uma remoção explícita vence o empate para não ressuscitar um
  jogo apagado offline. Empates entre upserts usam JSON canônico; títulos
  sintéticos (`steam:<id>`/`Steam <id>`) perdem para títulos reais;
- **playtime:** o pull aplica `max(local, remoto)` ao total exibido. O push
  continua enviando apenas deltas; o servidor os acumula, então minutos jogados
  em duas máquinas não são sobrescritos por um pull atrasado.

`resolveSyncConflict(kind, local, remote)` é o ponto de entrada genérico; as
funções específicas (`resolveAchievementConflict`,
`resolveLibraryConflict` e `resolvePlaytimeConflict`) ficam disponíveis no
contrato compartilhado. Entradas com chaves diferentes ou dados inválidos não
são combinadas.

## Offline e troca de conta

Conquistas desbloqueadas offline são persistidas em
`contas/<username>/sync_queue.json` (ou na raiz enquanto guest). A fila é
atômica, deduplicada por `(appid, apiname)` e só é drenada após o RPC retornar
sucesso. Falhas de rede deixam a fila intacta para retry com backoff.

`sync.js` captura o escopo da conta antes de push/pull e verifica o escopo após
a resposta da rede. Se ocorrer logout/login durante um RPC, o resultado antigo
é descartado e não é gravado na conta nova. O `queueLen` exibido pela UI e os
handlers IPC existentes permanecem compatíveis.

Para biblioteca e playtime, `sync_state.json` mantém watermarks por conta:
`libPush` e `playtimePush`. Um push offline não avança esses watermarks; a
próxima tentativa recalcula o delta a partir dos arquivos locais. O merge puro
acima torna o pull monotônico e convergente.

## Testes

```sh
node --test app/test/sync-resolver.test.js app/test/sync.test.js
```

Os testes de biblioteca (`biblioteca-pull.test.js` e
`biblioteca-remocao.test.js`) também cobrem a integração do resolver com o
pull real do main process.
