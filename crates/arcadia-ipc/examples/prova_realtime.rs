// Prova real do realtime: conecta num servidor Phoenix de mentira, faz o join,
// recebe dois postgres_changes e confere que o sink foi chamado.
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use arcadia_ipc::realtime::{RealtimeChannel, RECONEXAO};

#[tokio::main(flavor = "current_thread")]
async fn main() {
    let porta = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "8765".to_string());
    let base = format!("http://127.0.0.1:{porta}");

    let recebidos = Arc::new(AtomicUsize::new(0));
    let contador = recebidos.clone();

    let canal = Arc::new(RealtimeChannel::new(&base, "friends-teste", "token-abc"));
    println!("ws_url: {}", canal.ws_url());

    canal.on_change(move |payload| {
        let n = contador.fetch_add(1, Ordering::SeqCst) + 1;
        println!(
            "  evento #{n}: tabela={} requester={}",
            payload
                .get("table")
                .and_then(|v| v.as_str())
                .unwrap_or("?"),
            payload
                .get("new")
                .and_then(|v| v.get("requester_id"))
                .and_then(|v| v.as_str())
                .unwrap_or("?"),
        );
    });

    let eventos_ui = Arc::new(AtomicUsize::new(0));
    let ui = eventos_ui.clone();
    let sink: arcadia_ipc::EventSink = Arc::new(move |evento: &str, _payload: serde_json::Value| {
        ui.fetch_add(1, Ordering::SeqCst);
        println!("  sink -> {evento}");
    });

    canal.subscribe(sink);

    // Espera os eventos chegarem (o servidor fecha depois de mandar dois).
    for _ in 0..40 {
        if recebidos.load(Ordering::SeqCst) >= 2 {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }

    let total = recebidos.load(Ordering::SeqCst);
    let total_ui = eventos_ui.load(Ordering::SeqCst);
    canal.unsubscribe();

    // Espera um pouco para garantir que nada reconecta depois do unsubscribe.
    let antes = recebidos.load(Ordering::SeqCst);
    tokio::time::sleep(RECONEXAO + std::time::Duration::from_millis(400)).await;
    let depois = recebidos.load(Ordering::SeqCst);

    println!("\nresultado:");
    println!("  handlers chamados : {total}");
    println!("  eventos para a UI : {total_ui}");
    println!("  apos unsubscribe  : {antes} -> {depois} (nao pode crescer)");

    let ok = total == 2 && total_ui == 2 && antes == depois;
    println!("{}", if ok { "PROVADO" } else { "FALHOU" });
    std::process::exit(if ok { 0 } else { 1 });
}
