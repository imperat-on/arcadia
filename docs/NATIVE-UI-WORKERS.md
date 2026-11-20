# Workers do frontend nativo

Regras para agentes que trabalham nos crates `arcadia-native*` em worktrees do
Orca. Elas existem porque três builds de iced/wgpu em paralelo, cada um com o
próprio `target/`, saturam a máquina e parecem travados no terminal.

## Builds

Sempre use o wrapper:

```bash
scripts/native-worker-preflight.sh cargo check -p arcadia-native
scripts/native-worker-preflight.sh cargo test -p arcadia-native
```

Ele compartilha o `target/` do worktree principal, limita `CARGO_BUILD_JOBS`
para 4 e aplica timeout de 15 minutos. Ajuste com `ARCADIA_CMD_TIMEOUT`,
`CARGO_BUILD_JOBS` ou `ARCADIA_TARGET_DIR` se precisar.

## Comandos que não podem travar

- Nunca rode `cargo run`/GUI em foreground sem timeout. Use
  `timeout 20s cargo run ...` e trate o timeout como sucesso esperado para
  capturar screenshot.
- Não canalize comandos longos para `tail`/`tee` (o buffer esconde o
  progresso e o terminal parece parado). Deixe o Cargo escrever direto ou
  redirecione para um log e leia o arquivo.
- `cargo test` completo é para o fim da tarefa. Durante a iteração prefira
  `cargo check -p <crate>`.
- Se um comando passar de 10 minutos, interrompa e reporte o log; não fique
  esperando em silêncio.

## Escopo

- O worker só edita os arquivos do seu contrato. Não mexa em `src-tauri`,
  `ui/`, nem em crates de outra onda.
- Não commite `Cargo.lock`; o coordenador regenera e commita.
- Commits no branch do worktree, sem push.
