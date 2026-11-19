// Prova real do resolvedor: consulta o backend oficial e um host que só o DoH
// conhece. Roda fora do crate para não virar teste de rede na suíte.
use std::sync::Arc;

use arcadia_api::config::ApiConfig;
use arcadia_api::dns::BackendResolver;
use arcadia_api::http::{HttpClient, HttpRequest};

#[tokio::main]
async fn main() {
    let resolver = BackendResolver::new();

    // 1. Resolução do backend oficial (caminho do sistema normalmente).
    let inicio = std::time::Instant::now();
    match resolver.resolver("arcadiaserver.zesmehentperu.workers.dev", 443).await {
        Ok(ips) => println!(
            "sistema/DoH  workers.dev -> {:?}  ({} ms)",
            ips,
            inicio.elapsed().as_millis()
        ),
        Err(erro) => println!("FALHOU workers.dev: {erro}"),
    }

    // 2. Mesma consulta de novo: precisa vir do cache e ser instantânea.
    let inicio = std::time::Instant::now();
    match resolver.resolver("arcadiaserver.zesmehentperu.workers.dev", 443).await {
        Ok(ips) => println!(
            "cache        workers.dev -> {:?}  ({} µs)",
            ips,
            inicio.elapsed().as_micros()
        ),
        Err(erro) => println!("FALHOU cache: {erro}"),
    }
    println!("hosts em cache: {}", resolver.cache_len());

    // 3. Um host que resolve pelo sistema.
    match resolver.resolver("github.com", 443).await {
        Ok(ips) => println!("sistema      github.com -> {:?}", ips),
        Err(erro) => println!("FALHOU github.com: {erro}"),
    }

    // 4. Host inexistente: precisa falhar limpo, sem travar.
    let inicio = std::time::Instant::now();
    match resolver
        .resolver("nao-existe-mesmo-arcadia-teste.invalid", 443)
        .await
    {
        Ok(ips) => println!("INESPERADO resolveu invalido: {ips:?}"),
        Err(erro) => println!(
            "invalido falhou como esperado: {erro} ({} ms)",
            inicio.elapsed().as_millis()
        ),
    }

    // 5. HttpClient de verdade usando o resolvedor — prova o encaixe no reqwest.
    let config = Arc::new(ApiConfig::default());
    let client = HttpClient::with_timeout(config, std::time::Duration::from_secs(20)).unwrap();
    let pedido = HttpRequest::get(format!(
        "{}/auth/v1/health",
        arcadia_api::config::DEFAULT_API_URL
    ));
    match client.send(&pedido).await {
        Ok(resposta) => println!(
            "HttpClient   GET /auth/v1/health -> {}  ({} bytes)",
            resposta.status,
            resposta.body.len()
        ),
        Err(erro) => println!("HttpClient FALHOU: {erro:?}"),
    }
}
