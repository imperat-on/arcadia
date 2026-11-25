"use strict"

// Escala de UI do Arcadia: UM multiplicador AUTOMÁTICO sobre a base de cada skin.
//
// A interface se adapta sozinha à tela. O fator vem do tamanho da janela/monitor
// em pixels LÓGICOS com 1920x1080 de referência — e pixel lógico já embute a
// escala do sistema (um 4K a 150% chega aqui como ~2560x1440), então quem usa
// escala aumentada recebe a interface no tamanho certo sem configurar nada.
//
// Não existe mais controle manual de escala nem de tamanho de capa: eram duas
// preferências que o usuário podia deixar pior do que o automático, e a única
// razão para mantê-las era setups incomuns. Para esses, a válvula de escape é a
// variável de ambiente ARCADIA_UI_SCALE (ver escalaEfetiva).
//
// Histórico completo: ui_scale, console_ui_scale, desktop_scale_base_v2,
// desktop_font_scale_v3, big_picture_scale_defaults_v2/v3. A migração v5 aposenta
// de vez `ui_scale` e `card_scale`, que viravam números guardados sem efeito.

const BASE_POR_SKIN = { console: 1.3, desktop: 1.2 }

// Resolução lógica de referência: o layout foi desenhado nela.
const REFERENCIA = { width: 1920, height: 1080 }

// Faixa do multiplicador automático. O piso em 1 mantém o comportamento antigo
// (janela pequena nunca encolhe a interface) e o teto é generoso de propósito:
// em telas grandes o mesmo fator deixava a interface fisicamente pequena.
const ESCALA_MIN = 1
const ESCALA_MAX = 1.7

// Faixa da sobreposição por ambiente (ARCADIA_UI_SCALE), mais larga que a
// automática: é a válvula de escape para setups incomuns.
const MANUAL_MIN = 0.7
const MANUAL_MAX = 2

// Faixa do fator final entregue ao webContents.setZoomFactor
// (base 1.3 x teto 1.7 = 2.21, então o teto acomoda a skin do console).
const FATOR_MIN = 0.7
const FATOR_MAX = 2.4

// Marca a migração que aposenta as preferências de escala/capa.
const CHAVE_MIGRADA = "ui_scale_auto_v5"

// Chaves que a migração v5 aposenta. Todas eram valores guardados que só sabiam
// DIMINUIR a qualidade da adaptação (o automático cobre a mesma faixa), e as
// marcadoras v4 seguem o caminho.
const CHAVES_APOSENTADAS = [
  "ui_scale",
  "card_scale",
  "console_ui_scale",
  "desktop_scale_base_v2",
  "desktop_font_scale_v3",
  "big_picture_scale_defaults_v2",
  "big_picture_scale_defaults_v3",
  "ui_scale_v4",
]

function skinDoModo(mode) {
  return mode === "console" ? "console" : "desktop"
}

function baseDaSkin(mode) {
  return BASE_POR_SKIN[skinDoModo(mode)]
}

/**
 * Multiplicador automático para um tamanho em pixels lógicos.
 * Usa a MENOR dimensão: em ultrawide, escalar pela largura estouraria a altura.
 */
function escalaAutomatica(tamanho) {
  const largura = Number(tamanho?.width)
  const altura = Number(tamanho?.height)
  const w = Number.isFinite(largura) && largura > 0 ? largura : REFERENCIA.width
  const h = Number.isFinite(altura) && altura > 0 ? altura : REFERENCIA.height
  const razao = Math.min(w / REFERENCIA.width, h / REFERENCIA.height)
  if (!Number.isFinite(razao) || razao <= 0) return 1
  return Math.min(ESCALA_MAX, Math.max(ESCALA_MIN, razao))
}

/**
 * Sobreposição manual por ambiente (`ARCADIA_UI_SCALE`). Devolve null quando não
 * há sobreposição válida — inclusive "auto", que é o mesmo que não definir nada.
 * Serve para o caso raro (TV distante, monitor de 4K de 15 polegadas) sem
 * devolver ao usuário um controle que ele possa usar contra si mesmo.
 */
function escalaManual(valor) {
  if (valor === undefined || valor === null) return null
  const texto = String(valor).trim()
  if (texto === "" || texto.toLowerCase() === "auto") return null
  const n = Number(texto)
  if (!Number.isFinite(n) || n <= 0) return null
  return Math.min(MANUAL_MAX, Math.max(MANUAL_MIN, n))
}

/** Escala efetiva: o ambiente manda quando existe; senão, a automática. */
function escalaEfetiva(tamanho, ambiente) {
  const manual = escalaManual(ambiente)
  return manual === null ? escalaAutomatica(tamanho) : manual
}

/** Fator final para o webContents.setZoomFactor (base da skin × escala efetiva). */
function fatorDeZoom(mode, escalaDaTela = 1) {
  const escala = Number(escalaDaTela)
  const bruto = baseDaSkin(mode) * (Number.isFinite(escala) && escala > 0 ? escala : 1)
  return Math.min(FATOR_MAX, Math.max(FATOR_MIN, bruto))
}

/**
 * Tamanho da capa no trilho do Big Picture, derivado da MESMA escala da tela.
 * Devolve o multiplicador que o GameRail aplica sobre a largura base de 142px.
 * No padrão (escala 1) dá 1.6 — exatamente o tamanho de antes.
 */
function escalaDaCapa(escala) {
  const e = Number(escala)
  const base = Number.isFinite(e) && e > 0 ? e : 1
  return Math.min(1.9, Math.max(0.9, 1.6 * base))
}

/**
 * Migração de uma vez só: aposenta as preferências de escala/capa.
 *
 * NÃO preserva o tamanho que o usuário escolheu — é isso que a mudança quer:
 * quem tinha ajustado na mão passa a receber a escala automática da tela dele.
 * Devolve o patch, ou null quando não há nada a fazer.
 */
function migrarEscala(config) {
  const cfg = config && typeof config === "object" ? config : {}
  if (cfg[CHAVE_MIGRADA] === true) return null
  const patch = { [CHAVE_MIGRADA]: true }
  for (const chave of CHAVES_APOSENTADAS) patch[chave] = undefined
  return patch
}

module.exports = {
  BASE_POR_SKIN,
  REFERENCIA,
  ESCALA_MIN,
  ESCALA_MAX,
  MANUAL_MIN,
  MANUAL_MAX,
  FATOR_MIN,
  FATOR_MAX,
  CHAVE_MIGRADA,
  CHAVES_APOSENTADAS,
  skinDoModo,
  baseDaSkin,
  escalaAutomatica,
  escalaManual,
  escalaEfetiva,
  fatorDeZoom,
  escalaDaCapa,
  migrarEscala,
}
