// Prova o bug do avatar: o arquivo existe, mas o path devolvido pelo
// `pickImage` carrega `?t=<stamp>` e o read falha. Depois do fix, funciona.
use std::path::PathBuf;

#[tokio::main(flavor = "current_thread")]
async fn main() {
    let data_dir = std::env::var("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("/home/zes"))
        .join(".local/share/arcadia");

    // O arquivo que o pickImage realmente gravou nesta máquina.
    let alvo = ["avatar.gif", "avatar.jpg", "avatar.png"]
        .iter()
        .map(|nome| data_dir.join(nome))
        .find(|caminho| caminho.is_file());

    let Some(alvo) = alvo else {
        println!("nenhum avatar em {} — rode um pickImage primeiro", data_dir.display());
        std::process::exit(1);
    };

    let stamp = 1795120259552u128;
    let como_a_ui_manda = format!("file://{}?t={stamp}", alvo.display());

    println!("arquivo real ....... {}", alvo.display());
    println!("UI manda ........... {como_a_ui_manda}");
    println!();

    // O que o codigo ANTIGO fazia: strip_prefix e read direto.
    let antigo = como_a_ui_manda
        .strip_prefix("file://")
        .unwrap_or(&como_a_ui_manda);
    match std::fs::read(antigo) {
        Ok(bytes) => println!("ANTES ............... leu {} bytes (inesperado)", bytes.len()),
        Err(erro) => println!("ANTES ............... FALHA: {erro}  <- o bug"),
    }

    // O que o codigo NOVO faz: limpa query e hash.
    let limpo = limpar(&como_a_ui_manda);
    match std::fs::read(&limpo) {
        Ok(bytes) => {
            println!("DEPOIS .............. leu {} bytes de {}", bytes.len(), limpo);
            let formato = if bytes.starts_with(b"\x89PNG") {
                "PNG"
            } else if bytes.starts_with(b"GIF8") {
                "GIF"
            } else if bytes.starts_with(b"\xff\xd8\xff") {
                "JPEG"
            } else if bytes.len() > 12 && &bytes[8..12] == b"WEBP" {
                "WEBP"
            } else {
                "desconhecido"
            };
            println!("formato detectado ... {formato}");

            if formato == "desconhecido" {
                println!("\nFALHOU: formato nao reconhecido");
                std::process::exit(1);
            }
            println!("\nPROVADO");
        }
        Err(erro) => {
            println!("DEPOIS .............. FALHA: {erro}");
            std::process::exit(1);
        }
    }
}

/// Espelha `caminho_de_arquivo` do crate.
fn limpar(entrada: &str) -> String {
    let sem_esquema = entrada.strip_prefix("file://").unwrap_or(entrada);
    let sem_query = sem_esquema
        .split('?')
        .next()
        .unwrap_or(sem_esquema)
        .split('#')
        .next()
        .unwrap_or(sem_esquema);
    sem_query.to_string()
}
