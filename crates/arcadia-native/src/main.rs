//! Arcadia Native — frontend em Rust puro (iced) do shell desktop.
//!
//! O app chama os serviços do Arcadia diretamente em um runtime Tokio.
//! A verificação de fontes usa um navegador em processo separado.

mod animated_cards;
mod asset_cache;
mod app;
mod backend;
mod big_picture;
mod covers;
mod i18n;
mod input;
mod media;
mod picker;
mod rich_text;
mod timing;
mod shell;
mod source_browser;
mod theme;
mod toast;
mod views;

use std::path::{Path, PathBuf};
use std::sync::Arc;

use arcadia_ipc::AppState;
use iced::{window, Size};

use crate::app::App;
use crate::backend::Backend;

fn bundled_torrent_worker(executable: Option<&Path>) -> Option<PathBuf> {
    let worker = "resources/torrent_rpc/main.py";
    executable
        .and_then(Path::parent)
        .map(|dir| dir.join(worker))
        .into_iter()
        .chain([Path::new(env!("CARGO_MANIFEST_DIR")).join("../..").join(worker)])
        .find(|path| path.is_file())
}

fn main() -> iced::Result {
    let data_dir = arcadia_store::paths::resolve_data_dir();
    if let Err(error) = std::fs::create_dir_all(&data_dir) {
        eprintln!(
            "[arcadia-native] não foi possível criar o data dir {}: {error}",
            data_dir.display()
        );
    }

    let api = match arcadia_api::ApiClient::from_env() {
        Ok(api) => Some(Arc::new(api)),
        Err(error) => {
            eprintln!("[arcadia-native] backend sem sessão ({error}); usando biblioteca local");
            None
        }
    };

    let executable = std::env::current_exe().ok();
    let worker = bundled_torrent_worker(executable.as_deref());
    let mut state = AppState::with_runtime_resources(data_dir.clone(), api, worker);

    if let Some(executable) = executable {
        state.set_source_challenge_solver(Arc::new(source_browser::SourceBrowser::new(
            executable,
            data_dir.join("cache/source-browser"),
            arcadia_store::config::read_config(&data_dir)
                .get("language").and_then(serde_json::Value::as_str).unwrap_or("pt-BR").to_string(),
        )));
    }

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

    let downloads = Arc::clone(&state.downloads);
    let backend = Backend::new(state, runtime.handle().clone());
    // O monitor drena o progresso dos processos Steam/Epic e emite os eventos
    // consumidos pela interface, como no bootstrap Tauri.
    downloads.start_monitor();
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
            size: Size::new(1280.0, 800.0),
            min_size: Some(Size::new(1024.0, 640.0)),
            decorations: false,
            ..window::Settings::default()
        })
        .antialiasing(true)
        .run();

    let _ = downloads.shutdown();
    // O runtime é do `main`, não de uma task: encerrar aqui garante que o drop
    // bloqueante do tokio aconteça fora de um contexto async.
    drop(runtime);
    result
}
