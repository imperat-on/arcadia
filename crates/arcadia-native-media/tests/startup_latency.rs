//! Medição opt-in com uma URI real, sem fixar latência de rede na CI.
use arcadia_native_media::{Player, PlayerState};
use std::time::{Duration, Instant};

#[test]
#[ignore = "requer GStreamer e ARCADIA_STARTUP_URI"]
fn first_frame_latency() {
    let uri = std::env::var("ARCADIA_STARTUP_URI").expect("ARCADIA_STARTUP_URI");
    let mut latencies = Vec::new();
    for round in 0..3 {
        let mut player = Player::new().expect("GStreamer");
        let start = Instant::now();
        player.open(&uri).expect("abrir trailer");
        let frame = loop {
            if let Some(frame) = player.try_frame() {
                break frame;
            }
            if let PlayerState::Error(error) = player.state() {
                panic!("{error}");
            }
            assert!(
                start.elapsed() < Duration::from_secs(20),
                "trailer sem primeiro frame"
            );
            std::thread::sleep(Duration::from_millis(5));
        };
        let ms = start.elapsed().as_millis();
        println!(
            "FIRST_FRAME round={round} ms={ms} size={}x{}",
            frame.width, frame.height
        );
        latencies.push(ms);
    }
    latencies.sort();
    println!("FIRST_FRAME_MEDIAN_MS={}", latencies[1]);
}
