// Valida os manifests gerados com o MESMO parser que o updater usa.
// Se isto falhar, o auto-update do app quebraria no release publicado.
use std::env;
use std::path::PathBuf;

fn main() {
    let dir = env::args()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/tmp/rel160/flat"));

    let mut falhas = 0;

    for arquivo in ["latest-linux.yml", "latest.yml"] {
        let caminho = dir.join(arquivo);
        let texto = match std::fs::read_to_string(&caminho) {
            Ok(texto) => texto,
            Err(erro) => {
                println!("FALHA {arquivo}: {erro}");
                falhas += 1;
                continue;
            }
        };
        match arcadia_updater::manifest::parse_latest(&texto) {
            Ok(manifest) => {
                println!(
                    "{arquivo}: versao={} arquivos={} sha512={}...",
                    manifest.version,
                    manifest.files.len(),
                    &manifest.sha512[..12.min(manifest.sha512.len())]
                );
                // O Rust tem que considerar 1.6.0 mais novo que 1.5.5.
                if !arcadia_updater::version::is_newer_version(&manifest.version, "1.5.5") {
                    println!("  FALHA: 1.6.0 nao e considerado mais novo que 1.5.5");
                    falhas += 1;
                }
                // E nao pode ser mais novo que ele mesmo.
                if arcadia_updater::version::is_newer_version(&manifest.version, "1.6.0") {
                    println!("  FALHA: 1.6.0 comparado consigo mesmo");
                    falhas += 1;
                }
            }
            Err(erro) => {
                println!("FALHA {arquivo}: {erro:?}");
                falhas += 1;
            }
        }
    }

    println!();
    if falhas == 0 {
        println!("PROVADO: os dois manifests passam no parser do updater");
    } else {
        println!("FALHOU: {falhas} problema(s)");
        std::process::exit(1);
    }
}
