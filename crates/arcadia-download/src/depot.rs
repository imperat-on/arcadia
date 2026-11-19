//! Regras do runtime DepotDownloader: comandos por depot, arquivo de chaves,
//! diretório de instalação e detecção de .NET — sem spawn.
//!
//! Oráculo congelado: `app/electron/depotdownloader.js` (release/arquivos
//! obrigatórios) e a parte de download de `app/electron/steamstore.js`
//! (`sanitizeInstallDir`, `dotnetBin`, `dotnetRuntimeCompativel`,
//! `prepareDownload`).
//!
//! O crate só monta argv e decide caminhos; quem escreve o keys file, cria o
//! destino e spawna `dotnet` é a borda (Tauri). O probe de filesystem é
//! injetado para que os testes não toquem em disco.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

pub const DEPOT_RELEASE_URL: &str =
    "https://github.com/bedrockuser/DepotDownloaderMod/releases/download/release-2025_06_17/DepotDownloader-framework.zip";
pub const DEPOT_RELEASE_SHA256: &str =
    "e49a8eec7f09cd9ebcf8890e1aaedced787741634f7d1380d8f57558a920238e";
pub const MAX_ARCHIVE_BYTES: u64 = 64 * 1024 * 1024;
pub const RELEASE_HOST_PREFIX: &str =
    "https://github.com/bedrockuser/DepotDownloaderMod/releases/download/";

pub const DLL_NAME: &str = "DepotDownloader.dll";
pub const DEFAULT_MAX_DOWNLOADS: u32 = 20;
pub const MIN_DOTNET_MAJOR: u32 = 9;
pub const DOTNET_ERROR: &str =
    ".NET 9+ runtime não encontrado. Instale em https://dotnet.microsoft.com/download";
pub const NO_MANIFESTS_ERROR: &str =
    "nenhum depot com .manifest local — o provedor entregou o .lua sem os manifestos";

/// Nomes do zip do fork -> nomes históricos usados pelo Arcadia.
pub const RENAMES: &[(&str, &str)] = &[
    ("DepotDownloaderMod.dll", "DepotDownloader.dll"),
    ("DepotDownloaderMod.deps.json", "DepotDownloader.deps.json"),
    (
        "DepotDownloaderMod.runtimeconfig.json",
        "DepotDownloader.runtimeconfig.json",
    ),
];

/// Arquivos que precisam existir (não vazios) para o runtime ser utilizável.
pub const REQUIRED_FILES: &[&str] = &[
    "DepotDownloader.dll",
    "DepotDownloader.deps.json",
    "DepotDownloader.runtimeconfig.json",
    "QRCoder.dll",
    "SteamKit2.dll",
    "System.IO.Hashing.dll",
    "ZstdSharp.dll",
    "protobuf-net.Core.dll",
    "protobuf-net.dll",
];

/// Probe injetado: a borda responde com a semântica de `lstat` + `isFile()` +
/// `size > 0` (regular, não symlink, não vazio). Nenhuma leitura acontece aqui.
pub trait DepotFsProbe {
    fn is_regular_non_empty_file(&self, path: &Path) -> bool;

    /// Saída de `dotnet --list-runtimes`, quando a borda já a coletou.
    fn dotnet_runtimes(&self) -> Option<String> {
        None
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DepotError(pub String);

impl std::fmt::Display for DepotError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for DepotError {}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DepotSpec {
    #[serde(rename = "depotId")]
    pub depot_id: String,
    #[serde(rename = "manifestId")]
    pub manifest_id: String,
    #[serde(default)]
    pub key: String,
    #[serde(default)]
    pub size: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DepotCredentials {
    pub username: String,
    pub password: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DepotDownloadRequest {
    pub appid: String,
    /// `installdir` cru; será sanitizado (nunca contém separadores).
    pub installdir: String,
    pub depots: Vec<DepotSpec>,
    pub steam_dir: PathBuf,
    pub tmp_dir: PathBuf,
    pub deps_dir: PathBuf,
    /// Binário .NET já resolvido (`dotnet` do PATH ou cópia local).
    pub dotnet: String,
    pub branch: Option<String>,
    pub os: Option<String>,
    pub credentials: Option<DepotCredentials>,
    pub max_downloads: u32,
    pub validate: bool,
}

impl DepotDownloadRequest {
    pub fn new(
        appid: impl Into<String>,
        steam_dir: PathBuf,
        tmp_dir: PathBuf,
        deps_dir: PathBuf,
        dotnet: impl Into<String>,
    ) -> Self {
        Self {
            appid: appid.into(),
            installdir: String::new(),
            depots: Vec::new(),
            steam_dir,
            tmp_dir,
            deps_dir,
            dotnet: dotnet.into(),
            branch: None,
            os: None,
            credentials: None,
            max_downloads: DEFAULT_MAX_DOWNLOADS,
            validate: true,
        }
    }

    pub fn with_install_dir(mut self, installdir: impl Into<String>) -> Self {
        self.installdir = installdir.into();
        self
    }

    pub fn with_depots(mut self, depots: Vec<DepotSpec>) -> Self {
        self.depots = depots;
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DepotCommand {
    pub cmd: String,
    pub args: Vec<String>,
    pub depot_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DepotPreparation {
    pub commands: Vec<DepotCommand>,
    pub dest: PathBuf,
    pub keys_file: PathBuf,
    /// Linhas `depotId;key` (sem newline final), exatamente como o oráculo.
    pub keys_contents: String,
    pub skipped: Vec<String>,
}

/// `sanitizeInstallDir`: remove traversal e caracteres inválidos; vazio vira
/// `"jogo"`. Note que `.`, `_`, espaço e `-` são permitidos pelo oráculo.
pub fn sanitize_install_dir(installdir: &str) -> String {
    let filtered: String = installdir
        .chars()
        .filter(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '.' | '_' | ' ' | '-')
        })
        .collect();
    let without_dots = filtered.replace("..", "");
    let trimmed = without_dots.trim();
    if trimmed.is_empty() {
        "jogo".to_string()
    } else {
        trimmed.to_string()
    }
}

pub fn keys_file_path(tmp_dir: &Path, appid: &str) -> PathBuf {
    tmp_dir.join(format!("keys_{appid}.vdf"))
}

pub fn manifest_file_path(
    tmp_dir: &Path,
    appid: &str,
    depot_id: &str,
    manifest_id: &str,
) -> PathBuf {
    tmp_dir
        .join(format!("manifest_{appid}"))
        .join(format!("{depot_id}_{manifest_id}.manifest"))
}

/// Conteúdo do arquivo `-depotkeys` (depots sem chave ficam de fora).
pub fn keys_file_contents(depots: &[DepotSpec]) -> String {
    depots
        .iter()
        .filter(|depot| !depot.key.is_empty())
        .map(|depot| format!("{};{}", depot.depot_id, depot.key))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Todos os arquivos obrigatórios do runtime existem e não estão vazios.
pub fn depotdownloader_installed(deps_dir: &Path, probe: &impl DepotFsProbe) -> bool {
    REQUIRED_FILES
        .iter()
        .all(|name| probe.is_regular_non_empty_file(&deps_dir.join(name)))
}

/// `dotnetRuntimeCompativel(stdout)`: alguma linha `Microsoft.NETCore.App`
/// com major >= 9.
pub fn dotnet_runtime_compatible(stdout: &str) -> bool {
    stdout.lines().any(|line| {
        let Some(rest) = line.strip_prefix("Microsoft.NETCore.App") else {
            return false;
        };
        if !rest.starts_with(char::is_whitespace) {
            return false;
        }
        let rest = rest.trim_start();
        let major: String = rest
            .chars()
            .take_while(|character| character.is_ascii_digit())
            .collect();
        major
            .parse::<u32>()
            .is_ok_and(|major| major >= MIN_DOTNET_MAJOR)
    })
}

/// Nome do `dotnet` do PATH no sistema atual.
pub fn default_dotnet_binary_name() -> &'static str {
    if cfg!(windows) {
        "dotnet.exe"
    } else {
        "dotnet"
    }
}

/// `dotnetBin()`: cópia local se existir, senão o binário do PATH.
pub fn dotnet_binary(
    local_binary: &Path,
    system_binary: &str,
    probe: &impl DepotFsProbe,
) -> String {
    if probe.is_regular_non_empty_file(local_binary) {
        local_binary.to_string_lossy().into_owned()
    } else {
        system_binary.to_string()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DotnetDetection {
    pub path: Option<String>,
    pub error: Option<String>,
}

impl DotnetDetection {
    pub fn is_ok(&self) -> bool {
        self.path.is_some()
    }
}

/// `ensureDotnet` sem spawn: aceita a cópia local ou o runtime do sistema
/// quando o probe fornece `dotnet --list-runtimes`.
pub fn detect_dotnet(
    local_binary: &Path,
    system_binary: &str,
    probe: &impl DepotFsProbe,
) -> DotnetDetection {
    if probe.is_regular_non_empty_file(local_binary) {
        return DotnetDetection {
            path: Some(local_binary.to_string_lossy().into_owned()),
            error: None,
        };
    }
    if probe
        .dotnet_runtimes()
        .is_some_and(|runtimes| dotnet_runtime_compatible(&runtimes))
    {
        return DotnetDetection {
            path: Some(system_binary.to_string()),
            error: None,
        };
    }
    DotnetDetection {
        path: None,
        error: Some(DOTNET_ERROR.to_string()),
    }
}

/// `prepareDownload`: um comando por depot, em sequência. Depots sem
/// `manifestId` ou sem `.manifest` local são pulados (o DepotDownloader
/// anônimo responde 401 para manifest requests antigos).
pub fn prepare_download(
    request: &DepotDownloadRequest,
    probe: &impl DepotFsProbe,
) -> Result<DepotPreparation, DepotError> {
    let install_dir = sanitize_install_dir(&request.installdir);
    let dest = request
        .steam_dir
        .join("steamapps")
        .join("common")
        .join(install_dir);
    let keys_file = keys_file_path(&request.tmp_dir, &request.appid);
    let keys_contents = keys_file_contents(&request.depots);
    let dll = request.deps_dir.join(DLL_NAME);
    let dll = dll.to_string_lossy().into_owned();
    let mut commands = Vec::new();
    let mut skipped = Vec::new();
    for depot in &request.depots {
        if depot.manifest_id.is_empty() {
            skipped.push(depot.depot_id.clone());
            continue;
        }
        let manifest = manifest_file_path(
            &request.tmp_dir,
            &request.appid,
            &depot.depot_id,
            &depot.manifest_id,
        );
        if !probe.is_regular_non_empty_file(&manifest) {
            skipped.push(depot.depot_id.clone());
            continue;
        }
        let mut args = vec![
            dll.clone(),
            "-app".to_string(),
            request.appid.clone(),
            "-depot".to_string(),
            depot.depot_id.clone(),
            "-manifest".to_string(),
            depot.manifest_id.clone(),
        ];
        if let Some(branch) = request.branch.as_deref().filter(|value| !value.is_empty()) {
            args.push("-branch".to_string());
            args.push(branch.to_string());
        }
        if let Some(os) = request.os.as_deref().filter(|value| !value.is_empty()) {
            args.push("-os".to_string());
            args.push(os.to_string());
        }
        if let Some(credentials) = &request.credentials {
            args.push("-username".to_string());
            args.push(credentials.username.clone());
            args.push("-password".to_string());
            args.push(credentials.password.clone());
        }
        args.push("-manifestfile".to_string());
        args.push(manifest.to_string_lossy().into_owned());
        args.push("-depotkeys".to_string());
        args.push(keys_file.to_string_lossy().into_owned());
        args.push("-max-downloads".to_string());
        args.push(request.max_downloads.to_string());
        args.push("-dir".to_string());
        args.push(dest.to_string_lossy().into_owned());
        if request.validate {
            args.push("-validate".to_string());
        }
        commands.push(DepotCommand {
            cmd: request.dotnet.clone(),
            args,
            depot_id: depot.depot_id.clone(),
        });
    }
    if commands.is_empty() {
        return Err(DepotError(NO_MANIFESTS_ERROR.to_string()));
    }
    Ok(DepotPreparation {
        commands,
        dest,
        keys_file,
        keys_contents,
        skipped,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[derive(Default, Clone)]
    struct FakeProbe {
        files: HashSet<PathBuf>,
        runtimes: Option<String>,
    }

    impl FakeProbe {
        fn with_files(files: impl IntoIterator<Item = PathBuf>) -> Self {
            Self {
                files: files.into_iter().collect(),
                runtimes: None,
            }
        }
    }

    impl DepotFsProbe for FakeProbe {
        fn is_regular_non_empty_file(&self, path: &Path) -> bool {
            self.files.contains(path)
        }

        fn dotnet_runtimes(&self) -> Option<String> {
            self.runtimes.clone()
        }
    }

    fn depot(depot_id: &str, manifest_id: &str, key: &str) -> DepotSpec {
        DepotSpec {
            depot_id: depot_id.to_string(),
            manifest_id: manifest_id.to_string(),
            key: key.to_string(),
            size: 0,
        }
    }

    fn request() -> DepotDownloadRequest {
        DepotDownloadRequest::new(
            "440",
            PathBuf::from("C:/Steam"),
            PathBuf::from("C:/tmp"),
            PathBuf::from("C:/deps"),
            "dotnet.exe",
        )
        .with_install_dir("Team Fortress 2")
        .with_depots(vec![
            depot("111", "222", "chave-1"),
            depot("333", "444", ""),
        ])
    }

    #[test]
    fn sanitize_install_dir_remove_traversal_e_caracteres_invalidos() {
        assert_eq!(sanitize_install_dir("Team Fortress 2"), "Team Fortress 2");
        assert_eq!(sanitize_install_dir("../../evil"), "evil");
        assert_eq!(sanitize_install_dir("C:\\Games\\X"), "CGamesX");
        assert_eq!(sanitize_install_dir("jogo:com/barra*?"), "jogocombarra");
        assert_eq!(sanitize_install_dir("   "), "jogo");
        assert_eq!(sanitize_install_dir(""), "jogo");
        // O oráculo remove ".." mas deixa um "." resultante.
        assert_eq!(sanitize_install_dir("..."), ".");
        assert_eq!(sanitize_install_dir("a_b-c.d"), "a_b-c.d");
    }

    #[test]
    fn prepare_monta_um_comando_por_depot_com_argv_do_oraculo() {
        let request = request();
        let first_manifest = manifest_file_path(&request.tmp_dir, "440", "111", "222");
        let second_manifest = manifest_file_path(&request.tmp_dir, "440", "333", "444");
        let probe = FakeProbe::with_files([first_manifest.clone(), second_manifest.clone()]);
        let preparation = prepare_download(&request, &probe).unwrap();

        assert_eq!(preparation.commands.len(), 2);
        let dll = request
            .deps_dir
            .join(DLL_NAME)
            .to_string_lossy()
            .into_owned();
        let dest = request
            .steam_dir
            .join("steamapps")
            .join("common")
            .join("Team Fortress 2")
            .to_string_lossy()
            .into_owned();
        let keys = request
            .tmp_dir
            .join("keys_440.vdf")
            .to_string_lossy()
            .into_owned();
        assert_eq!(
            preparation.commands[0],
            DepotCommand {
                cmd: "dotnet.exe".to_string(),
                args: vec![
                    dll.clone(),
                    "-app".to_string(),
                    "440".to_string(),
                    "-depot".to_string(),
                    "111".to_string(),
                    "-manifest".to_string(),
                    "222".to_string(),
                    "-manifestfile".to_string(),
                    first_manifest.to_string_lossy().into_owned(),
                    "-depotkeys".to_string(),
                    keys.clone(),
                    "-max-downloads".to_string(),
                    "20".to_string(),
                    "-dir".to_string(),
                    dest.clone(),
                    "-validate".to_string(),
                ],
                depot_id: "111".to_string(),
            }
        );
        assert_eq!(
            preparation.dest,
            request
                .steam_dir
                .join("steamapps")
                .join("common")
                .join("Team Fortress 2")
        );
        assert_eq!(preparation.keys_file, request.tmp_dir.join("keys_440.vdf"));
        assert_eq!(preparation.keys_contents, "111;chave-1");
        assert!(preparation.skipped.is_empty());
    }

    #[test]
    fn depots_sem_manifest_local_ou_sem_manifest_id_sao_pulados() {
        let request = request();
        let first_manifest = manifest_file_path(&request.tmp_dir, "440", "111", "222");
        let probe = FakeProbe::with_files([first_manifest]);
        let preparation = prepare_download(&request, &probe).unwrap();
        assert_eq!(preparation.commands.len(), 1);
        assert_eq!(preparation.commands[0].depot_id, "111");
        assert_eq!(preparation.skipped, vec!["333".to_string()]);

        let mut without_manifest = request.clone();
        without_manifest.depots.push(depot("555", "", "x"));
        let preparation = prepare_download(&without_manifest, &probe).unwrap();
        assert_eq!(preparation.commands.len(), 1);
        assert!(preparation.skipped.contains(&"555".to_string()));
    }

    #[test]
    fn sem_nenhum_manifest_retorna_o_erro_do_oraculo() {
        let request = request();
        let probe = FakeProbe::default();
        assert_eq!(
            prepare_download(&request, &probe),
            Err(DepotError(NO_MANIFESTS_ERROR.to_string()))
        );
    }

    #[test]
    fn branch_os_credenciais_e_validate_sao_opcionais() {
        let mut request = request();
        request.branch = Some("public".to_string());
        request.os = Some("windows".to_string());
        request.credentials = Some(DepotCredentials {
            username: "user".to_string(),
            password: "pass".to_string(),
        });
        request.max_downloads = 4;
        request.validate = false;
        let manifest = manifest_file_path(&request.tmp_dir, "440", "111", "222");
        let probe = FakeProbe::with_files([manifest]);
        let preparation = prepare_download(&request, &probe).unwrap();
        let args = &preparation.commands[0].args;
        let branch = args.iter().position(|arg| arg == "-branch").unwrap();
        assert_eq!(args[branch + 1], "public");
        let os = args.iter().position(|arg| arg == "-os").unwrap();
        assert_eq!(args[os + 1], "windows");
        let user = args.iter().position(|arg| arg == "-username").unwrap();
        assert_eq!(args[user + 1], "user");
        let password = args.iter().position(|arg| arg == "-password").unwrap();
        assert_eq!(args[password + 1], "pass");
        let max = args.iter().position(|arg| arg == "-max-downloads").unwrap();
        assert_eq!(args[max + 1], "4");
        assert!(!args.iter().any(|arg| arg == "-validate"));
    }

    #[test]
    fn keys_file_ignora_depots_sem_chave() {
        let depots = vec![
            depot("1", "10", "a"),
            depot("2", "20", ""),
            depot("3", "30", "c"),
        ];
        assert_eq!(keys_file_contents(&depots), "1;a\n3;c");
        assert_eq!(keys_file_contents(&[]), "");
    }

    #[test]
    fn installed_exige_todos_os_arquivos_obrigatorios() {
        let deps = PathBuf::from("C:/deps");
        let all: Vec<PathBuf> = REQUIRED_FILES.iter().map(|name| deps.join(name)).collect();
        let probe = FakeProbe::with_files(all.clone());
        assert!(depotdownloader_installed(&deps, &probe));

        for missing in REQUIRED_FILES.iter().take(3) {
            let partial: Vec<PathBuf> = all
                .iter()
                .filter(|path| path.file_name().and_then(|name| name.to_str()) != Some(*missing))
                .cloned()
                .collect();
            let probe = FakeProbe::with_files(partial);
            assert!(!depotdownloader_installed(&deps, &probe), "{missing}");
        }
    }

    #[test]
    fn runtime_parser_aceita_apenas_netcore_app_major_9_ou_maior() {
        let output = "\
Microsoft.AspNetCore.App 9.0.7 [C:\\Program Files\\dotnet\\shared\\Microsoft.AspNetCore.App]
Microsoft.NETCore.App 8.0.7 [C:\\Program Files\\dotnet\\shared\\Microsoft.NETCore.App]
";
        assert!(!dotnet_runtime_compatible(output));
        assert!(dotnet_runtime_compatible(
            "Microsoft.NETCore.App 9.0.0 [C:\\dotnet]"
        ));
        assert!(dotnet_runtime_compatible(
            "Microsoft.NETCore.App 10.1.2 [C:\\dotnet]"
        ));
        assert!(!dotnet_runtime_compatible(
            "Microsoft.NETCore.App9.0.0 [C:\\dotnet]"
        ));
        assert!(!dotnet_runtime_compatible(""));
        assert!(!dotnet_runtime_compatible("  Microsoft.NETCore.App 9.0.0"));
    }

    #[test]
    fn detect_dotnet_prefere_copia_local_e_aceita_runtime_do_sistema() {
        let local = PathBuf::from("C:/deps/dotnet/dotnet.exe");
        let probe = FakeProbe::with_files([local.clone()]);
        let detection = detect_dotnet(&local, "dotnet.exe", &probe);
        assert_eq!(
            detection,
            DotnetDetection {
                path: Some(local.to_string_lossy().into_owned()),
                error: None,
            }
        );

        let probe = FakeProbe {
            files: HashSet::new(),
            runtimes: Some("Microsoft.NETCore.App 9.0.7 [C:\\dotnet]".to_string()),
        };
        assert_eq!(
            detect_dotnet(&local, "dotnet.exe", &probe),
            DotnetDetection {
                path: Some("dotnet.exe".to_string()),
                error: None,
            }
        );

        let probe = FakeProbe {
            files: HashSet::new(),
            runtimes: Some("Microsoft.NETCore.App 8.0.7 [C:\\dotnet]".to_string()),
        };
        let detection = detect_dotnet(&local, "dotnet.exe", &probe);
        assert!(!detection.is_ok());
        assert_eq!(detection.error.as_deref(), Some(DOTNET_ERROR));
    }

    #[test]
    fn dotnet_binary_usa_local_quando_presente() {
        let local = PathBuf::from("C:/deps/dotnet/dotnet");
        let probe = FakeProbe::with_files([local.clone()]);
        assert_eq!(
            dotnet_binary(&local, "dotnet", &probe),
            local.to_string_lossy().into_owned()
        );
        assert_eq!(
            dotnet_binary(&local, "dotnet", &FakeProbe::default()),
            "dotnet"
        );
    }

    #[test]
    fn dest_fica_sempre_dentro_de_steamapps_common() {
        let mut request = request();
        request.installdir = "../../../tmp/evil".to_string();
        let manifest = manifest_file_path(&request.tmp_dir, "440", "111", "222");
        let probe = FakeProbe::with_files([manifest]);
        let preparation = prepare_download(&request, &probe).unwrap();
        assert_eq!(
            preparation.dest,
            PathBuf::from("C:/Steam/steamapps/common/tmpevil")
        );
        assert!(!preparation.commands[0]
            .args
            .iter()
            .any(|arg| arg.contains("..")));
    }
}
