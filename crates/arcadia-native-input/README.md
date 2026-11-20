# arcadia-native-input

Entrada de gamepad e navegação por foco para o frontend nativo em Rust do
Arcadia. Usa `gilrs` diretamente, faz polling não bloqueante e não depende de
iced nem de webview.

O crate é o equivalente nativo de
`ui/src/components/ps5-launcher/useGamepadNav.ts`: traduz botões e eixos para
intenções (`NavEvent`) e oferece modelos de foco (`FocusGrid` e `FocusList`)
que a UI atualiza sem precisar de hardware.

## Uso

```rust
use std::time::Duration;
use arcadia_native_input::{EdgeBehavior, FocusGrid, NavEvent, NavInput};

let mut input = NavInput::new();
let mut grid = FocusGrid::with_edge(7, 4, EdgeBehavior::Wrap);

// Uma vez por quadro; o delta alimenta a repetição das direções.
for event in input.tick(Duration::from_millis(16)) {
    match event {
        NavEvent::Confirm => { /* abrir grid.index() */ }
        NavEvent::Back => { /* voltar/fechar */ }
        NavEvent::PageLeft => { /* aba anterior */ }
        NavEvent::PageRight => { /* próxima aba */ }
        NavEvent::Menu => { /* menu */ }
        NavEvent::Options => { /* opções */ }
        NavEvent::Quit => { /* sair */ }
        direction => {
            grid.apply(direction);
        }
    }
}
```

`tick` nunca bloqueia e devolve um vetor vazio quando não há evento. Sem
controle conectado — ou sem backend gilrs disponível — tudo continua
funcionando, só não chegam eventos.

## API pública

| Item | Papel |
|---|---|
| `NavEvent` | `Up`, `Down`, `Left`, `Right`, `Confirm`, `Back`, `PageLeft`, `PageRight`, `Menu`, `Options`, `Quit`. `delta()` devolve o passo em coordenadas de tela. |
| `NavInput` | Dono do backend gilrs. `new`, `with_config`, `headless`, `tick(dt)`, `is_connected`, `gamepad_name`, `config`/`config_mut`, `set_mapping`, `set_repeat`, `set_deadzone`. |
| `InputConfig` | `ButtonMap` + `RepeatConfig` + `deadzone` (padrão `0.6`). |
| `ButtonMap` | Botão gilrs de cada ação. Campos públicos para remapear. |
| `RepeatConfig` | `enabled`, `initial_delay` (500 ms) e `repeat_interval` (260 ms). `RepeatConfig::disabled()` desliga a repetição. |
| `FocusGrid` | Grade `cols × rows` com `move_up/down/left/right`, `apply(NavEvent)`, `set_index`, `row`, `col`, `index`. |
| `FocusList` | Lista vertical com `move_up/down`, `apply(NavEvent)`, `set_index`, `index`. |
| `EdgeBehavior` | `Clamp` (padrão) para nas bordas; `Wrap` dá a volta na linha/coluna. |

O índice de foco em `FocusGrid` é linear (`row * cols + col`) e sempre menor
que `len()`. Grades e listas vazias mantêm o índice em `0` e ignoram
movimentos. `Wrap` na grade anda dentro da própria linha na horizontal e da
própria coluna na vertical; na lista, dá a volta nas pontas.

## Mapeamento padrão

| Ação | Botão gilrs | Padrão web |
|---|---|---|
| `Confirm` | `South` | A / botão 0 |
| `Back` | `East` | B / botão 1 |
| `PageLeft` | `LeftTrigger` (L1/LB) | botão 4 |
| `PageRight` | `RightTrigger` (R1/RB) | botão 5 |
| `Menu` | `Start` | botão 9 |
| `Options` | `Select` | botão 8 |
| `Quit` | `Mode` (Guide/PS) | — |
| `Up`, `Down`, `Left`, `Right` | `DPadUp`, `DPadDown`, `DPadLeft`, `DPadRight` | botões 12–15 |

O analógico esquerdo é lido de `Axis::LeftStickX/Y`, com zona morta de `0.6`
(mesmo `DEADZONE_PADRAO` da UI web). D-pad tem prioridade sobre o analógico.
Em diagonal, o eixo dominante vence; empate fica com o vertical.

Remapeamento:

```rust
use arcadia_native_input::{ButtonMap, InputConfig, NavInput};
use gilrs::Button;

let mut config = InputConfig::default();
config.buttons.confirm = Button::East; // confirma com Circle
config.buttons.back = Button::West;
config.repeat.initial_delay = std::time::Duration::from_millis(400);
config.repeat.repeat_interval = std::time::Duration::from_millis(200);
config.deadzone = 0.5;

let mut input = NavInput::with_config(config);

// Ou em tempo de execução:
let mut map = ButtonMap::standard();
map.page_left = Button::LeftTrigger2; // L2/LT
input.set_mapping(map);
```

`Button::LeftTrigger` é o bumper; `LeftTrigger2` é o gatilho analógico. Evite
mapear o mesmo botão para duas ações: as direções são verificadas antes das
ações e a primeira correspondência vence. `quit` é `Option<Button>`, então
`None` desliga o botão de sair.

## Repetição

- O primeiro evento sai imediatamente no aperto.
- Segurando, o próximo sai depois de `initial_delay` e os demais a cada
  `repeat_interval`.
- No máximo um repeat por `tick`, mesmo que o quadro tenha sido longo; um
  travamento não faz o foco atravessar vários itens.
- Trocar de direção reinicia o temporizador. Soltar interrompe a repetição.
- Ações (`Confirm`, `Back`, páginas, `Menu`, `Options`, `Quit`) nunca repetem.

## Integração na UI nativa

1. Crie **um** `NavInput` no estado da aplicação, não dentro do render. O tipo
   não é sincronizado internamente; mantenha-o na thread do loop de UI.
2. A cada quadro, chame `input.tick(dt)` com o delta real (ou 16 ms fixos) e
   trate os eventos antes de desenhar.
3. Use `FocusGrid` para grades de capas e `FocusList` para menus/colunas. Para
   navegação espacial com tiles de tamanhos diferentes, o renderer pode aplicar
   a mesma ideia do `bestInDirection` web; os modelos prontos cobrem o caso
   determinístico de grade/lista.
4. `PageLeft`/`PageRight` trocam abas ou páginas; `Menu`/`Options` abrem
   overlays; `Back` fecha o overlay atual. A UI decide a precedência quando há
   modal aberto.
5. Ao perder o foco da janela ou abrir um modal que captura a entrada, chame
   `input.reset()` para descartar direções seguradas e eventos pendentes. A
   desconexão de um controle também limpa o estado automaticamente.
6. Para testes e ambientes sem controle, use `NavInput::headless()`: `tick`
   devolve vazio e a configuração/foco continuam testáveis.

## Testes

```sh
cargo test -p arcadia-native-input
```

A suíte cobre bordas, wrap, grades/listas vazias, mapeamento, zona morta,
histerese, prioridade do D-pad, diagonal e temporização de repetição. Nenhum
teste exige gamepad conectado.
