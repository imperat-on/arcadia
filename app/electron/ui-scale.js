"use strict"

// Escala de UI do Arcadia: UM multiplicador relativo sobre a base de cada skin.
//
// Histórico (5 chaves para a mesma coisa):
//   ui_scale, console_ui_scale, desktop_scale_base_v2, desktop_font_scale_v3,
//   big_picture_scale_defaults_v2, big_picture_scale_defaults_v3
// Dois problemas concretos: (1) o zoom do desktop IGNORAVA `ui_scale` — a base
// era fixa em 1.2, então o valor gravado não tinha efeito nenhum na tela; (2) cada
// skin tinha o seu número solto, e "100%" queria dizer coisas diferentes.
//
// Agora `ui_scale` é o multiplicador (1 = 100%) e a base da skin entra na conta.
// Nos valores padrão o fator final continua idêntico ao de antes — desktop 1.2,
// console 1.3 —, então ninguém acorda com a interface maior ou menor.

const BASE_POR_SKIN = { console: 1.3, desktop: 1.2 }
const REL_MIN = 0.7
const REL_MAX = 1.6
const FATOR_MIN = 0.7
const FATOR_MAX = 2

// Marca a migração das chaves antigas. Depois dela, só `ui_scale` existe.
const CHAVE_MIGRADA = "ui_scale_v4"

// Chaves que a migração aposenta. `ui_scale` fica; o resto sai do config.
const CHAVES_LEGADAS = [
  "console_ui_scale",
  "desktop_scale_base_v2",
  "desktop_font_scale_v3",
  "big_picture_scale_defaults_v2",
  "big_picture_scale_defaults_v3",
]

function skinDoModo(mode) {
  return mode === "console" ? "console" : "desktop"
}

function baseDaSkin(mode) {
  return BASE_POR_SKIN[skinDoModo(mode)]
}

/** 100% = 1. Inválido cai em 1; fora da faixa é preso na faixa. */
function clampRelativo(valor) {
  const n = Number(valor)
  if (!Number.isFinite(n) || n <= 0) return 1
  return Math.min(REL_MAX, Math.max(REL_MIN, n))
}

/** Fator final para o webContents.setZoomFactor (base da skin × relativo × janela). */
function fatorDeZoom(mode, relativo, escalaDaJanela = 1) {
  const janela = Number(escalaDaJanela)
  const bruto =
    baseDaSkin(mode) *
    clampRelativo(relativo) *
    (Number.isFinite(janela) && janela > 0 ? janela : 1)
  return Math.min(FATOR_MAX, Math.max(FATOR_MIN, bruto))
}

/**
 * Migração de uma vez só das chaves antigas para `ui_scale`.
 *
 * - `console_ui_scale` customizado vira o relativo (dividido pela base do
 *   console), então quem já tinha ajustado o Big Picture vê o mesmo tamanho;
 * - o `ui_scale` antigo do desktop era decorativo (o zoom o ignorava) e volta
 *   para 1 — é justamente o que mantém o tamanho atual na tela;
 * - os marcadores de migração saem do config (patch `undefined` some no
 *   JSON.stringify do writeConfig).
 *
 * Devolve o patch, ou null quando não há nada a fazer.
 */
function migrarEscala(config) {
  const cfg = config && typeof config === "object" ? config : {}
  if (cfg[CHAVE_MIGRADA] === true) return null

  const patch = { [CHAVE_MIGRADA]: true }
  const legadoConsole = Number(cfg.console_ui_scale)
  const consoleCustomizado =
    Number.isFinite(legadoConsole) &&
    legadoConsole > 0 &&
    Math.abs(legadoConsole - BASE_POR_SKIN.console) > 0.001

  patch.ui_scale = consoleCustomizado
    ? clampRelativo(legadoConsole / BASE_POR_SKIN.console)
    : 1
  for (const chave of CHAVES_LEGADAS) patch[chave] = undefined
  return patch
}

module.exports = {
  BASE_POR_SKIN,
  REL_MIN,
  REL_MAX,
  CHAVE_MIGRADA,
  CHAVES_LEGADAS,
  skinDoModo,
  baseDaSkin,
  clampRelativo,
  fatorDeZoom,
  migrarEscala,
}
