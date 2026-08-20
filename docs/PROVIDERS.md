# Providers locais do indexador

`index.py` mantém a descoberta no disco e `indexers/` transforma cada registro em
um jogo do contrato da biblioteca. Os transformadores não executam runners, nãoazem rede e podem ser validados com as fixtures em `fixtures/providers/`.

## Steam

- `steamapps/appmanifest_<appid>.acf` é lido pelo parser KeyValues; somente o
  bloco `AppState` é convertido.
- `steamapps/libraryfolders.vdf` fornece bibliotecas adicionais. O fallback do
  parser aceita blocos na mesma linha, comentários `//` e strings escapadas,
  necessários para instalações reais sem `python-vdf`.
- Arte local vem de `appcache/librarycache/<appid>/` (inclusive subpastas de
  hash). Runtimes e redistribuíveis são filtrados por ID/nome.

## Legendary e Heroic

Legendary usa `list-games --json` e `list-installed --json`; respostas em lista ou
no envelope `games`/`data` são aceitas. O cache do Heroic fica em
`~/.config/heroic/store_cache/<runner>_library.json`, com runners `gog`,
`legendary` e `nile`. O título e as artes são normalizados, e valores de
instalação textuais como `"false"` não são tratados como verdadeiros.

## Lutris

O indexador consulta somente linhas `installed = 1` de
`~/.local/share/lutris/pga.db`. Jogos cujo serviço é Steam e cujo `service_id`
já aparece na biblioteca Steam são deduplicados; os demais usam o URI
`lutris:rungameid/<id>`.

## SLSsteam

`AdditionalApps` é lido do `config.yaml`. A lista suporta IDs sem aspas ou com
aspas, comentários e lista YAML inline; IDs inválidos são ignorados. Entradas
repetidas, já indexadas ou de runtimes Steam não são emitidas pelo provider.

## Fixtures e golden tests

As entradas reais reduzidas e os envelopes esperados ficam em
`fixtures/providers/<provider>/`. Rode a suíte Python no checkout: 

```sh
python3 -m unittest discover -s . -p 'test_*.py'
```

Os testes comparam os transformadores com os `golden.json`, além de cobrirem a
leitura de ACF/VDF, caches JSON, linhas Lutris e YAML do SLSsteam.
