use std::path::PathBuf;

fn main() {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }

    println!("cargo:rerun-if-changed=icon.rc");
    println!("cargo:rerun-if-changed=../arcadia-native/assets/icons/icon.ico");

    // O SDK do Windows normalmente é achado pelo embed-resource, mas algumas
    // instalações têm a versão mais nova do SDK sem rc.exe. Apontar o RC para
    // uma versão que tenha a ferramenta evita falhar o build por causa de um
    // componente cosmético.
    if std::env::var_os("RC").is_none() {
        if let Some(rc) = find_windows_rc() {
            std::env::set_var("RC", rc);
        }
    }

    match embed_resource::compile("icon.rc", embed_resource::NONE).manifest_optional() {
        Ok(()) => {}
        Err(error) if std::env::var("PROFILE").as_deref() == Ok("release") => {
            panic!("não foi possível embutir o ícone do Arcadia no release: {error:?}");
        }
        Err(error) => {
            println!("cargo:warning=ícone do Arcadia não embutido neste build: {error:?}");
        }
    }
}

fn find_windows_rc() -> Option<PathBuf> {
    let arch = match std::env::var("CARGO_CFG_TARGET_ARCH").as_deref() {
        Ok("aarch64") => "arm64",
        Ok("x86") => "x86",
        _ => "x64",
    };

    for variable in ["ProgramFiles(x86)", "ProgramFiles"] {
        let Some(root) = std::env::var_os(variable) else {
            continue;
        };
        let kits = PathBuf::from(root)
            .join("Windows Kits")
            .join("10")
            .join("bin");
        let Ok(entries) = std::fs::read_dir(&kits) else {
            continue;
        };

        let mut versions: Vec<PathBuf> = entries
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter(|path| path.is_dir())
            .collect();
        versions.sort();
        versions.reverse();

        for version in versions {
            let candidate = version.join(arch).join("rc.exe");
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }

    None
}
