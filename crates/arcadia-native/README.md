# arcadia-native

Primeira fatia vertical do frontend nativo do Arcadia: Rust puro com `iced`,
sem Tauri e sem webview. O app Tauri (`src-tauri`) e a UI React (`ui/`) continuam
sendo a referência e não são tocados por este crate.

## O que já funciona

- `AppState` + `dispatch_async("library:get")` reais, num runtime tokio próprio.
- Biblioteca com capas baixadas por `reqwest`, decodificadas pelo crate `image`
  e servidas ao iced como `image::Handle` em cache de memória.
- Sidebar de navegação, lista de jogos, busca e grade responsiva de cards.
- Página do jogo com hero, metadados, descrição em texto puro, horas e áreas
  reservadas para Trailer/Conquistas/Controle.
- Estados de carregamento, biblioteca vazia, erro de IPC/rede e placeholder de
  capa. Nenhuma falha de rede vira panic.

## Módulos

| Arquivo | Papel |
|---|---|
| `main.rs` | Bootstrap: data dir real, `AppState`, runtime tokio, janela 1600×900 |
| `backend.rs` | Ponte IPC, view model `Game`, parsing de `library:get`, download de capas |
| `shell.rs` | `arcadia_ipc::Shell` para o app nativo |
| `app.rs` | Estado Elm, navegação, `Task`s assíncronas |
| `theme.rs` | Tokens de `ui/src/index.css` e estilos dos widgets |
| `covers.rs` | Cache de capas/heroes em memória |
| `views/sidebar.rs` | Sidebar de ~240 px |
| `views/library.rs` | Grade da biblioteca |
| `views/game.rs` | Página do jogo |
| `media.rs` | Stub do player GStreamer |
| `input.rs` | Stub do gamepad |

## Rodar

```sh
cargo run -p arcadia-native
```

O backend usa `arcadia_store::paths::resolve_data_dir()` e, quando
`ApiClient::from_env()` funciona, restaura a sessão como o bootstrap Tauri. Sem
sessão, a biblioteca global/guest continua carregando.

## Próximas ondas

1. Player GStreamer em `media.rs` (trailers HLS) e vídeo de boot.
2. Input/gamepad em `input.rs` + Big Picture.
3. Telas de Lojas, Downloads, Fontes, Amigos e Configurações.
4. Tema com Inter embarcada: trocar as constantes `FONT*` em `theme.rs` e
   registrar a fonte no `Application`.
