//! Arcadia Native — frontend em Rust puro (iced) do shell desktop.
//!
//! Sem Tauri e sem webview: `AppState` e `dispatch_async` do `arcadia-ipc` são
//! chamados diretamente, num runtime tokio próprio, e a biblioteca real vira
//! widgets nativos. `src-tauri` e `ui/` continuam intocados.

mod app;
mod backend;
mod covers;
mod input;
mod media;
mod shell;
mod theme;
mod views;

use std::sync::Arc;

use arcadia_ipc::AppState;
use iced::{window, Size};

use crate::app::App;
use crate::backend::Backend;

fn main() -> iced::Result {
    let data_dir = arcadia_store::paths::resolve_data_dir();
    if let Err(error) = std::fs::create_dir_all(&data_dir) {
        eprintln!(
            "[arcadia-native] não foi possível criar o data dir {}: {error}",
            data_dir.display()
        );
    }

    // Mesma escolha do bootstrap Tauri: se o cliente do backend estiver
    // configurado, ele restaura a sessão e a biblioteca escopada por conta.
    // Sem ele, o app ainda abre com a biblioteca global/guest.
    let state = match arcadia_api::ApiClient::from_env() {
        Ok(api) => AppState::with_api(data_dir.clone(), Arc::new(api)),
        Err(error) => {
            eprintln!("[arcadia-native] backend sem sessão ({error}); usando biblioteca local");
            AppState::new(data_dir.clone())
        }
    };

    let runtime = match tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .thread_name("arcadia-native")
        .build()
    {
        Ok(runtime) => runtime,
        Err(error) => {
            eprintln!("[arcadia-native] falha ao criar o runtime tokio: {error}");
            std::process::exit(1);
        }
    };

    let backend = Backend::new(state, runtime.handle().clone());
    eprintln!(
        "[arcadia-native] data dir: {}",
        backend.data_dir().display()
    );

    let result = iced::application(move || App::boot(backend.clone()), App::update, App::view)
        .title("ARCADIA")
        .theme(theme::arcadia())
        .subscription(App::subscription)
        .default_font(theme::FONT)
        .font(include_bytes!("../assets/fonts/Inter-Regular.ttf").as_slice())
        .font(include_bytes!("../assets/fonts/Inter-Medium.ttf").as_slice())
        .font(include_bytes!("../assets/fonts/Inter-SemiBold.ttf").as_slice())
        .font(include_bytes!("../assets/fonts/Inter-Bold.ttf").as_slice())
        .window(window::Settings {
            size: Size::new(1600.0, 900.0),
            min_size: Some(Size::new(960.0, 540.0)),
            decorations: false,
            ..window::Settings::default()
        })
        .antialiasing(true)
        .run();

    // O runtime é do `main`, não de uma task: encerrar aqui garante que o drop
    // bloqueante do tokio aconteça fora de um contexto async.
    drop(runtime);
    result
}
