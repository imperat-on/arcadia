//! Contrato de empacotamento Linux.
//!
//! Estes testes existem porque os erros de empacotamento deste projeto não
//! aparecem no build: eles aparecem na máquina do usuário, como trailer em tela
//! preta ou app que não abre. Cada asserção aqui trava uma decisão que já
//! quebrou uma vez.
//!
//! Nada aqui compila binário nem empacota — lê os manifests versionados e
//! confere invariantes. Roda em qualquer host, inclusive no CI sem webkit.

use std::fs;
use std::path::{Path, PathBuf};

fn repo_root() -> PathBuf {
    // CARGO_MANIFEST_DIR = <raiz>/crates/arcadia-updater
    // ancestors: 0 = próprio crate, 1 = crates/, 2 = raiz
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("raiz do repositório")
        .to_path_buf()
}

fn tauri_conf() -> serde_json::Value {
    let path = repo_root().join("src-tauri/tauri.conf.json");
    let raw = fs::read_to_string(&path).unwrap_or_else(|e| panic!("lendo {}: {e}", path.display()));
    serde_json::from_str(&raw).unwrap_or_else(|e| panic!("parse de {}: {e}", path.display()))
}

fn workspace_version() -> String {
    let path = repo_root().join("Cargo.toml");
    let raw = fs::read_to_string(&path).expect("lendo Cargo.toml");
    for line in raw.lines() {
        let line = line.trim();
        if let Some(rest) = line.strip_prefix("version = ") {
            return rest.trim().trim_matches('"').to_string();
        }
    }
    panic!("versão do workspace não encontrada em Cargo.toml");
}

#[test]
fn versao_do_bundle_casa_com_o_workspace() {
    // Um bundle 0.1.0 num release 1.5.5 faz o updater nunca encontrar a versão
    // publicada — o app acha que já está atualizado e para de checar.
    let conf = tauri_conf();
    let bundle_version = conf["version"].as_str().expect("version no tauri.conf");
    assert_eq!(
        bundle_version,
        workspace_version(),
        "a versão do tauri.conf.json divergiu do workspace; os nomes dos \
         artefatos sairiam trocados"
    );
}

#[test]
fn bundle_inclui_alvos_linux() {
    let conf = tauri_conf();
    let targets = conf["bundle"]["targets"]
        .as_array()
        .expect("bundle.targets como lista");
    let names: Vec<&str> = targets.iter().filter_map(|t| t.as_str()).collect();

    // "nsis" sozinho é Windows: num repo que publica AppImage, o Linux ficaria
    // sem artefato nenhum.
    for expected in ["deb", "appimage"] {
        assert!(
            names.contains(&expected),
            "bundle.targets não inclui '{expected}': {names:?}"
        );
    }
}

#[test]
fn deb_declara_as_dependencias_de_runtime() {
    let conf = tauri_conf();
    let depends = conf["bundle"]["linux"]["deb"]["depends"]
        .as_array()
        .expect("bundle.linux.deb.depends como lista");
    let joined = depends
        .iter()
        .filter_map(|d| d.as_str())
        .collect::<Vec<_>>()
        .join(" ");

    // Sem webkit o binário nem abre; sem os plugins de GStreamer o vídeo fica
    // preto. Ambos já foram um bug real neste projeto.
    for required in ["webkit2gtk-4.1", "gtk-3", "gst-plugins-base"] {
        assert!(
            joined.contains(required),
            "deb.depends não menciona '{required}': {joined}"
        );
    }

    // H.264 vem de gst-plugins-bad (openh264) ou de gst-libav. Um dos dois tem
    // que estar declarado, senão nenhum trailer toca.
    assert!(
        joined.contains("gst-plugins-bad") || joined.contains("gst-libav"),
        "deb.depends não cobre H.264 (nem gst-plugins-bad nem gst-libav): {joined}"
    );
}

#[test]
fn rpm_declara_as_dependencias_de_runtime() {
    // O RPM saía sem nenhuma dependência de GStreamer: no Fedora, uma máquina
    // limpa instalaria o app e todo trailer viraria tela preta. Os nomes de
    // pacote do RPM são outros (gstreamer1-*), então a lista é separada da do
    // deb e precisa existir por si.
    let conf = tauri_conf();
    let depends = conf["bundle"]["linux"]["rpm"]["depends"]
        .as_array()
        .expect("bundle.linux.rpm.depends como lista");
    let joined = depends
        .iter()
        .filter_map(|d| d.as_str())
        .collect::<Vec<_>>()
        .join(" ");

    for required in ["webkit2gtk", "gtk3", "gstreamer1-plugins-base"] {
        assert!(
            joined.contains(required),
            "rpm.depends não menciona '{required}': {joined}"
        );
    }
    assert!(
        joined.contains("gstreamer1-plugins-bad") || joined.contains("gstreamer1-libav"),
        "rpm.depends não cobre H.264: {joined}"
    );
}

#[test]
fn before_build_command_resolve_da_raiz_do_workspace() {
    let conf = tauri_conf();
    let cmd = conf["build"]["beforeBuildCommand"]
        .as_str()
        .expect("beforeBuildCommand");

    // O Tauri executa esse comando na raiz do workspace, não em src-tauri/.
    // Um "--prefix ../ui" aponta para fora do repo e quebra todo build.
    assert!(
        !cmd.contains("../ui"),
        "beforeBuildCommand usa caminho relativo quebrado: {cmd}"
    );
    assert!(
        cmd.contains("ui"),
        "beforeBuildCommand não aponta para a UI: {cmd}"
    );
}

#[test]
fn frontend_dist_aponta_para_a_ui() {
    let conf = tauri_conf();
    let dist = conf["build"]["frontendDist"]
        .as_str()
        .expect("frontendDist");
    assert!(
        dist.contains("ui/dist"),
        "frontendDist deveria apontar para ui/dist: {dist}"
    );
}

#[test]
fn script_de_empacotamento_embarca_codecs_de_video() {
    // O bundleMediaFramework do Tauri embarca os plugins básicos mas deixa
    // openh264 e hls de fora. O script compensa isso; se alguém remover essa
    // compensação, o AppImage volta a sair com trailer preto.
    let script = repo_root().join("scripts/package-linux.sh");
    let raw = fs::read_to_string(&script).unwrap_or_else(|e| panic!("lendo script: {e}"));

    for needed in ["libgstopenh264.so", "libgsthls.so", "libopenh264.so.8"] {
        assert!(
            raw.contains(needed),
            "scripts/package-linux.sh não injeta '{needed}' — trailer ficaria preto"
        );
    }
}

#[test]
fn torrent_worker_vai_nos_recursos_do_bundle() {
    // O worker Python do torrent não existe no data_dir do usuário: no Electron
    // ele vinha de `__dirname/torrent_rpc`. Sem embarcar main.py e
    // torrent_downloader.py (o segundo é importado pelo primeiro), o download
    // por torrent falha ao conectar em toda instalação empacotada.
    //
    // `bundle.resources` aceita lista (caminhos preservados) ou mapa
    // (origem -> destino achatado). O projeto usa mapa desde que o binário
    // `arcadia-gamepad` passou a viajar junto: achatado, ele fica em
    // `resources/arcadia-gamepad` em vez de replicar `target/release/`.
    let conf = tauri_conf();
    let recursos = &conf["bundle"]["resources"];
    let joined = match recursos.as_array() {
        Some(lista) => lista
            .iter()
            .filter_map(|r| r.as_str())
            .collect::<Vec<_>>()
            .join(" "),
        None => recursos
            .as_object()
            .expect("bundle.resources como lista ou mapa")
            .keys()
            .map(String::as_str)
            .collect::<Vec<_>>()
            .join(" "),
    };

    for required in ["torrent_rpc/main.py", "torrent_rpc/torrent_downloader.py"] {
        assert!(
            joined.contains(required),
            "bundle.resources não embarca '{required}': {joined}"
        );
    }
}

#[test]
fn binario_do_gamepad_vai_no_pacote() {
    // O wrapper XInput agora é Rust. Se ele não viajar no bundle, o launcher
    // detecta o controle e não tem o que executar — o jogo abre sem XInput,
    // que é justamente o bug que o wrapper existe para evitar.
    let conf = tauri_conf();
    let recursos = &conf["bundle"]["resources"];
    let joined = match recursos.as_array() {
        Some(lista) => lista
            .iter()
            .filter_map(|r| r.as_str())
            .collect::<Vec<_>>()
            .join(" "),
        None => recursos
            .as_object()
            .expect("bundle.resources como lista ou mapa")
            .keys()
            .map(String::as_str)
            .collect::<Vec<_>>()
            .join(" "),
    };
    assert!(
        joined.contains("arcadia-gamepad"),
        "bundle.resources não embarca o wrapper XInput: {joined}"
    );
}

#[test]
fn script_windows_aponta_o_resource_compiler_do_mingw() {
    // Sem RC apontando para o windres, o tauri-winres tenta o rc.exe do Windows
    // SDK e aborta o build inteiro no Linux.
    let script = repo_root().join("scripts/package-windows.sh");
    let raw = fs::read_to_string(&script).unwrap_or_else(|e| panic!("lendo script: {e}"));
    assert!(
        raw.contains("RC_x86_64_pc_windows_gnu"),
        "package-windows.sh não exporta RC_x86_64_pc_windows_gnu — o build cruzado quebra no tauri-winres"
    );
    assert!(
        raw.contains("x86_64-w64-mingw32-windres"),
        "package-windows.sh não aponta o windres do mingw"
    );
}

#[test]
fn cargo_config_nao_fixa_rc_absoluto() {
    // Um RC absoluto do Windows SDK em .cargo/config.toml quebra qualquer host
    // que não seja aquela máquina exata — Linux, CI, ou outro SDK.
    let path = repo_root().join(".cargo/config.toml");
    if !path.is_file() {
        return;
    }
    let raw = fs::read_to_string(&path).expect("lendo .cargo/config.toml");
    for line in raw.lines() {
        let line = line.trim();
        if line.starts_with('#') || !line.starts_with("RC") {
            continue;
        }
        assert!(
            !line.contains(":\\") && !line.contains("Windows Kits"),
            "RC absoluto do Windows SDK fixado no config versionado: {line}"
        );
    }
}

#[test]
fn verificador_de_pacote_existe_e_e_acionavel() {
    let verify = repo_root().join("scripts/verify-package.sh");
    assert!(
        verify.is_file(),
        "scripts/verify-package.sh sumiu — sem ele o CI não detecta pacote quebrado"
    );
    let raw = fs::read_to_string(&verify).expect("lendo verificador");
    // O teste de decode é o que prova que o pacote funciona sozinho.
    assert!(
        raw.contains("openh264dec"),
        "verificador não testa decode H.264"
    );
}
