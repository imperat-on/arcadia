# arcadia-native

Interface do Arcadia em Rust com iced. O mesmo binário abre a interface
Desktop e o Big Picture, com navegação por controle.

```sh
cargo run --locked -p arcadia-native
cargo test --locked -p arcadia-native
```

Consulte o [guia de empacotamento](../../docs/NATIVE-PACKAGING.md) para instalar
as dependências de compilação ou gerar os pacotes para Linux e Windows.

| Módulo | Responsabilidade |
|---|---|
| `main.rs` | Inicialização, runtime Tokio e janela |
| `app.rs`, `big_picture.rs` | Estado, navegação e ações assíncronas |
| `backend.rs`, `shell.rs` | Comandos e eventos de `arcadia-ipc` |
| `views/`, `theme.rs` | Telas iced, componentes e estilos |
| `covers.rs`, `asset_cache.rs` | Carregamento e cache de imagens |
| `media.rs`, `input.rs` | Reprodução de vídeo e suporte a controle |
| `source_browser.rs` | Verificação interativa de fontes em processo isolado no Linux |

`ARCADIA_DATA_DIR` seleciona o diretório de dados. A sessão salva restaura a
conta na abertura. Sem sessão, o Arcadia carrega a biblioteca local.
