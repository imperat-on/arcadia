// Chama o cache de capas com as URLs reais do espelho do catálogo e mede o
// ganho total de uma página de 24 jogos.
use std::time::Instant;

use arcadia_ipc::cover_cache::CoverCache;

#[tokio::main(flavor = "current_thread")]
async fn main() {
    let dir = std::env::temp_dir().join(format!("covers-pagina-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let cache = std::sync::Arc::new(CoverCache::new(&dir));

    // Pega as capas do espelho real do catálogo.
    let espelho = dirs_home().join(".local/share/arcadia/catalog_espelho");
    let mut arquivos: Vec<_> = std::fs::read_dir(&espelho)
        .map(|e| e.filter_map(Result::ok).collect())
        .unwrap_or_default();
    arquivos.sort_by_key(|e| e.file_name());

    let mut urls: Vec<String> = Vec::new();
    for arquivo in arquivos {
        let Ok(texto) = std::fs::read_to_string(arquivo.path()) else {
            continue;
        };
        let Ok(valor) = serde_json::from_str::<serde_json::Value>(&texto) else {
            continue;
        };
        let Some(games) = valor.get("games").and_then(|g| g.as_array()) else {
            continue;
        };
        for game in games {
            if let Some(url) = game.get("cover").and_then(|c| c.as_str()) {
                if url.starts_with("http") && !urls.contains(&url.to_string()) {
                    urls.push(url.to_string());
                }
            }
        }
        if urls.len() >= 24 {
            break;
        }
    }
    urls.truncate(24);

    if urls.is_empty() {
        println!("sem URLs no espelho; nada a medir");
        std::process::exit(1);
    }

    println!("capas da pagina: {}", urls.len());

    // Tamanho dos originais (HEAD, sem baixar).
    let cliente = reqwest::Client::new();
    let mut bytes_originais = 0u64;
    for url in &urls {
        if let Ok(resposta) = cliente.head(url).send().await {
            bytes_originais += resposta
                .headers()
                .get("content-length")
                .and_then(|v| v.to_str().ok())
                .and_then(|v| v.parse::<u64>().ok())
                .unwrap_or(0);
        }
    }

    let inicio = Instant::now();
    let mut ok = 0usize;
    // Concorrência limitada, igual ao canal `retro:covers`: serial levava 89 s.
    use futures_util::stream::{self, StreamExt};
    let resultados: Vec<bool> = stream::iter(urls.clone())
        .map(|url| {
            let cache = std::sync::Arc::clone(&cache);
            async move { cache.obter(&url).await.is_some() }
        })
        .buffer_unordered(6)
        .collect()
        .await;
    ok += resultados.iter().filter(|sucesso| **sucesso).count();
    let tempo = inicio.elapsed();

    let bytes_cache = cache.bytes_em_disco();

    println!("baixadas com sucesso: {ok}/{}", urls.len());
    println!("originais ....... {:.1} MB", bytes_originais as f64 / 1048576.0);
    println!("cache reduzido ... {:.1} MB", bytes_cache as f64 / 1048576.0);
    if bytes_originais > 0 {
        println!(
            "economia ......... {:.0}%",
            100.0 * (1.0 - bytes_cache as f64 / bytes_originais as f64)
        );
    }
    println!("tempo 1a visita .. {:.1} s", tempo.as_secs_f64());

    // Segunda passada: tudo em disco.
    let inicio = Instant::now();
    for url in &urls {
        let _ = cache.obter(url).await;
    }
    println!("tempo 2a visita .. {:.0} ms", inicio.elapsed().as_millis());
}

fn dirs_home() -> std::path::PathBuf {
    std::env::var("HOME")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| std::path::PathBuf::from("/home/zes"))
}
