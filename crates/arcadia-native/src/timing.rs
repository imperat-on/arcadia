//! Medição de latência por seção, só para diagnóstico.
//!
//! A Home, a Loja e o Retro dependem de canais IPC que fazem rede; sem número
//! não dá para saber se a lentidão é do servidor, do parse ou do render. Cada
//! `mark` guarda o instante em que o pedido saiu e `elapsed_ms` devolve quanto
//! demorou quando a resposta chega.
//!
//! O estado é global por chave (`Mutex<HashMap>`) porque as tasks de `update`
//! não têm um lugar natural para carregar o cronômetro — e o custo é um lock
//! por carga, não por frame.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::Instant;

static MARKS: Mutex<Option<HashMap<&'static str, Instant>>> = Mutex::new(None);

/// Marca o início de uma carga identificada por `key`.
pub fn mark(key: &'static str) {
    let mut guard = MARKS.lock().unwrap_or_else(|error| error.into_inner());
    guard
        .get_or_insert_with(HashMap::new)
        .insert(key, Instant::now());
}

/// Milissegundos desde o último [`mark`] da chave. `0` se não havia marca.
pub fn elapsed_ms(key: &'static str) -> u128 {
    let mut guard = MARKS.lock().unwrap_or_else(|error| error.into_inner());
    let Some(marks) = guard.as_mut() else {
        return 0;
    };
    marks
        .remove(key)
        .map(|start| start.elapsed().as_millis())
        .unwrap_or(0)
}
