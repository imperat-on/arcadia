# Platina Arcadia

A platina é um selo do Arcadia, calculado a partir dos desbloqueios da conta. Não é uma conquista enviada à Steam ou um item separado no servidor. Cada dispositivo reconstrói o selo quando recebe as conquistas sincronizadas. Importações e pulls não tocam o aviso de platina.

## Cobertura inicial

| AppID Steam | Jogo | Jogo base | DLC |
| --- | --- | ---: | ---: |
| 400 | Portal | 15 | — |
| 221910 | The Stanley Parable | 10 | — |
| 268910 | Cuphead | 28 | 14 (The Delicious Last Course) |
| 2012840 | Portal with RTX | 15 | — |

Os IDs vêm do catálogo público de conquistas da Steam (`https://steamcommunity.com/stats/<appid>/achievements/?l=english`). A divisão de Cuphead foi conferida com https://cuphead.wiki.gg/wiki/Achievements e com as descrições da Steam. Portal Soundtrack não tem conquistas. Portal with RTX publica os mesmos IDs de Portal, mas o progresso é separado por AppID. Jogos fora dessa lista continuam exibindo conquistas, sem selo automático.

## Manutenção

- Edite `app/electron/achievements/platinum.js` só quando a lista do jogo base estiver comprovada. Raridade, contagem total, posição no catálogo e prefixos do nome não identificam DLC.
- Use os IDs canônicos do AppID, não títulos traduzidos. Cada ID precisa aparecer em um só grupo. O DLC pode mudar sem alterar requisitos do jogo base.
- Uma entrada arquivada, ambígua ou não localizada não vira troféu conquistado por inferência. Se faltar uma data confiável, mostre o selo sem data.
- Um desbloqueio corrigido no runtime invalida a platina localmente. O pull com a mesma data não o restaura. O protocolo já existente exclui a linha no servidor, mas não retroage a exclusão em dispositivos que já haviam importado a linha: não há tombstone distribuído no protocolo atual.
- Rode `node --test test/trophy-platinum.test.js test/trophy-notifications.test.js` dentro de `app`.
