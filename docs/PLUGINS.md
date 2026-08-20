# Plugins locais e SDK do Arcadia

O Arcadia possui uma base local para integrações opcionais. Ela não é um
instalador: registrar um plugin apenas lê e valida os arquivos que o usuário já
colocou no disco. Nenhum pacote é baixado, descompactado ou executado pelo
registro.

## Pacote e manifest v1

Um pacote é um diretório que contém `plugin.json` e o arquivo `entry` indicado
por ele:

```json
{
  "manifestVersion": 1,
  "apiVersion": 1,
  "id": "com.example.overlay",
  "name": "Example overlay",
  "version": "1.0.0",
  "description": "Integração opcional para a biblioteca.",
  "entry": "index.js",
  "entrySha256": "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
  "permissions": ["library:read", "events:subscribe"]
}
```

- `manifestVersion` é a versão do formato (atualmente `1`); `apiVersion` é a
  versão do contrato do host (atualmente `1`). Os dois são obrigatórios para
  plugins de disco.
- `id` é único, minúsculo, e aceita apenas `[a-z0-9._-]` (1–64 caracteres).
- `version` usa semver (`MAJOR.MINOR.PATCH`, com prerelease/build opcionais).
- `entry` é relativo ao diretório do pacote. Caminhos absolutos, `..`,
  separadores Windows, NUL e symlinks que saiam do pacote são rejeitados.
- `entrySha256` é opcional e, quando presente, deve ser exatamente o SHA-256
  (64 dígitos hexadecimais) dos bytes do `entry`; `entryDigest` e as grafias
  snake_case correspondentes são aceitas como aliases de leitura e sempre
  canonizadas para `entrySha256`.
- `permissions` é uma lista sem curingas, duplicatas ou permissões
  desconhecidas. Campos extras também são rejeitados para não virar uma forma
  acidental de configurar o processo principal.

As grafias legadas `manifest_version`, `schemaVersion`, `api_version` e
`entrypoint` são aceitas na leitura e sempre convertidas para o formato
canônico acima. Um manifest de versão futura não é interpretado como v1.

### Permissões v1

| Permissão | Escopo reservado |
| --- | --- |
| `library:read` / `library:write` | ler/alterar biblioteca |
| `games:read` / `games:launch` | consultar/lançar jogos |
| `events:subscribe` | assinar eventos públicos |
| `commands:register` | registrar comandos públicos |
| `network` | rede do adaptador do plugin |
| `filesystem:read` / `filesystem:write` | arquivos mediados pelo host |
| `process:spawn` | processos externos mediados pelo host |
| `notifications` | notificações do Arcadia |
| `settings:read` / `settings:write` | preferências públicas |

Declarar uma permissão não concede acesso a Node/Electron. O host deve checar
a permissão antes de entregar cada capability; o SDK fornece
`assertPermission` e `capability` para esse fim.

## Registro local

O registro canônico fica em:

```text
$ARCADIA_DATA_DIR/plugins/registry.json
```

O arquivo tem envelope versionado `{ "version": 1, "plugins": { ... } }`, é
escrito atomicamente com modo `0600` e guarda o caminho privado do pacote e o
estado `enabled`. A resposta enviada à UI nunca inclui esse caminho. O arquivo
histórico `bin/plugins.json` continua sendo lido e mantido como espelho apenas
com flags de ativação; isso permite atualizar instalações existentes sem
perder preferências.

`register` valida o diretório, `plugin.json`, a identidade do manifest e o
entry antes de gravar. A ativação inicial é desligada por padrão. Desativar ou
remover do registro não apaga o diretório do usuário. Built-ins (SLSsteam e
LuaTools) são integrações do host e não podem ser substituídos por um pacote
com o mesmo ID.

### Integridade do entry

O registro calcula o SHA-256 dos bytes do `entry` usando apenas operações de
arquivo; ele nunca faz `require`, importa ou executa o módulo. Se o manifest
declarar `entrySha256`, um valor diferente faz `register` falhar com
`entry_digest_mismatch`. O digest calculado também é guardado no registro
canônico no momento do registro. Assim, um pacote v1 sem digest continua
aceito, mas uma alteração posterior do arquivo ainda pode ser detectada pela
verificação; instalações antigas sem esse snapshot continuam compatíveis e
são reportadas como não verificadas.

A API somente leitura `registry.verify(id)` retorna `ok`/`valid`,
`verified`, `expectedDigest`, `actualDigest`, `algorithm: "sha256"` e um erro
sem expor o caminho privado do pacote. `registry.verifyPackage(directory)` faz
a mesma leitura antes do registro. Para entry ausente, não regular ou que
passe por symlink para fora do pacote, a verificação falha. Um digest divergente
marca o plugin como inválido e impede sua ativação; nenhum código do plugin é
executado durante `register`, `get`, `listDetailed` ou `verify`.

## API de host

`app/electron/plugins.js` mantém as chamadas existentes:

- `list()` — shape histórico (`id`, `name`, `descKey`, `installed`, `enabled`);
- `install(id)` / `remove(id)` — confirmar/desativar built-ins, sem download;
- `isEnabled(id)` — estado efetivo.

A superfície versionada adicional é:

- `listDetailed()` / `get(id)` / `manifest(id)`;
- `register(directory)` / `unregister(id)`;
- `enable(id)` / `disable(id)`;
- `permissions(id)` / `hasPermission(id, permission)`;
- `verify(id)` / `verifyPackage(directory)` — verifica SHA-256 sem executar o
  entry;
- `sdk(id)` — cria um contexto mínimo do SDK.

No Electron, o preload preserva `pluginsList`/`pluginsInstall`/`pluginsRemove` e
expõe os canais tipados `pluginsDetails`, `pluginsGet`, `pluginsRegister`,
`pluginsUnregister`, `pluginsEnable` e `pluginsDisable`. O main normaliza IDs e
caminhos, e as respostas continuam sem paths absolutos do registro.

O módulo não importa o `entry` automaticamente. A execução de código de
plugins e capabilities concretas deve ser adicionada em uma etapa posterior,
com isolamento e revisão própria.

## SDK mínimo

```js
const sdk = plugins.sdk("com.example.overlay")
const readLibrary = sdk.capability("library:read", () => hostLibrary.read())

// lança PluginPermissionError se o plugin estiver desativado ou sem a scope
const games = readLibrary()
```

A checagem ocorre no momento de cada chamada, portanto revogar uma permissão
ou desativar o plugin invalida capabilities já criadas. O renderer recebe
somente metadados e métodos explícitos via preload; `ipcRenderer`, `require`,
paths do registro e o módulo SDK não são expostos diretamente.

## Fluxo recomendado para uma integração

1. Distribua um diretório local com `plugin.json` e entry relativo.
2. Peça ao usuário para selecionar/registrar o diretório (não use URL recebida
   da UI como comando).
3. Mostre as permissões antes da ativação e habilite somente após confirmação.
4. Solicite uma capability pública por vez e trate `permission_denied`.
5. Ao desinstalar, remova o registro; nunca apague arquivos fora da pasta de
   dados sem confirmação explícita.

A assinatura de pacotes, rollback e marketplace permanecem etapas futuras. Até
lá, plugins locais devem ser tratados como código não confiável.
