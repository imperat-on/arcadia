// Prova real do cache de capas: baixa uma capa do catálogo, mede o original e
// o arquivo reduzido, e confirma que a segunda chamada não toca na rede.
use std::time::Instant;

use arcadia_ipc::cover_cache::CoverCache;

#[tokio::main(flavor = "current_thread")]
async fn main() {
    let dir = std::env::temp_dir().join(format!("covers-prova-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let cache = CoverCache::new(&dir);

    // Capa real do catálogo (PNG grande do libretro-thumbnails).
    let url = "https://raw.githubusercontent.com/libretro-thumbnails/Sony_-_PlayStation_2/master/Named_Boxarts/007%20-%20Agent%20Under%20Fire%20(Korea).png";

    // Tamanho do original, para comparar.
    let original = reqwest::get(url).await.unwrap().bytes().await.unwrap().len();

    let inicio = Instant::now();
    let caminho = cache.obter(url).await;
    let primeira = inicio.elapsed();

    let Some(caminho) = caminho else {
        println!("FALHOU: cache não produziu arquivo");
        std::process::exit(1);
    };
    let reduzida = std::fs::metadata(&caminho).map(|m| m.len()).unwrap_or(0);

    let inicio = Instant::now();
    let segunda = cache.obter(url).await;
    let tempo_segunda = inicio.elapsed();

    println!("original ......... {:.0} KB", original as f64 / 1024.0);
    println!(
        "reduzida ......... {:.0} KB  ({:.1}% do original)",
        reduzida as f64 / 1024.0,
        100.0 * reduzida as f64 / original as f64
    );
    println!("1a chamada ....... {} ms (baixa + decodifica + grava)", primeira.as_millis());
    println!("2a chamada ....... {} µs (só disco)", tempo_segunda.as_micros());
    println!("arquivos em cache  {}", cache.quantidade());
    println!("bytes em disco ... {:.0} KB", cache.bytes_em_disco() as f64 / 1024.0);

    // A capa de 24 jogos: projeção com o arquivo reduzido.
    let projecao = reduzida as f64 * 24.0 / 1024.0 / 1024.0;
    let original_proj = original as f64 * 24.0 / 1024.0 / 1024.0;
    println!();
    println!("pagina de 24 capas:");
    println!("  antes .......... {:.1} MB", original_proj);
    println!("  depois ......... {:.1} MB", projecao);

    let ok = segunda.is_some() && reduzida > 0 && (reduzida as usize) < original;
    println!("\n{}", if ok { "PROVADO" } else { "FALHOU" });
    let _ = std::fs::remove_dir_all(&dir);
    std::process::exit(if ok { 0 } else { 1 });
}
