# Navegação no renderer

O trilho de jogos do modo console usa **roving tabindex**. Somente a capa
selecionada tem `tabindex="0"`; as demais ficam em `tabindex="-1"`. Isso evita
que dezenas (ou centenas) de jogos apareçam na sequência de Tab e mantém o
foco do teclado e do controle no jogo que está visível.

## Controles

- `Tab` entra no trilho pela capa selecionada; `Shift+Tab` sai pelo item
  anterior.
- `←`/`→` movem a seleção e o foco; `Home` e `End` saltam para a primeira e
  a última capa. O trilho para nas extremidades (não há loop inesperado).
- `Enter`/`Espaço` ativam a capa focada. `↓` continua abrindo o overview do
  jogo, como antes.
- No gamepad, D-pad/analógico esquerdo continua controlando a seleção e o
  botão A continua lançando o jogo. Quando o trilho já tem foco, a capa
  selecionada acompanha a mudança feita pelo gamepad.

A navegação é limitada ao `GameRail` do console. A UI desktop mantém sua ordem
natural de Tab, mouse e atalhos existentes; nenhuma regra de seleção da
biblioteca desktop é compartilhada com o trilho console.
