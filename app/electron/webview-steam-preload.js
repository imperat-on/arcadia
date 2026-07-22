// Preload da <webview> que embute a loja web da Steam no Arcadia.
// Roda no contexto ISOLADO da página de terceiros: só fala com o host via
// ipcRenderer.sendToHost / .on — nunca expõe Node para a página da Steam.
//
// Única injeção: nas páginas de JOGO (/app/<id>) coloca uma barra discreta com
// "Baixar (Arcadia)" e "Adicionar à Steam". O clique vira mensagem para o host
// (StoreConsole.tsx), que dispara o fluxo de download do próprio Arcadia. Fora
// de página de jogo, nenhuma barra. Sem tema/CSS na página.
const { ipcRenderer } = require("electron")

// Rótulos traduzidos vindos do host (arcadia:labels). Padrões em pt.
let labels = {
  baixar: "Baixar (Arcadia)",
  adicionar: "Adicionar à Steam",
  remover: "Remover",
  restart: "Reiniciar Steam",
}
// Estado do jogo atual vindo do host (arcadia:estado).
let estado = { adicionado: false, ocupado: false }

const BAR_ID = "arcadia-actionbar"
const CHROME_ID = "arcadia-chrome"

// Esconde só a "casca" da Steam: header global (logo + LOJA/COMUNIDADE/SOBRE +
// Instale o Steam/login/idioma) e o rodapé (Steam/Valve). NÃO é tema — só
// remove essas duas faixas em todas as páginas da loja.
const CHROME_CSS = `
#global_header, .responsive_header, .responsive_header_content,
.header_installsteam_btn, #footer, #global_footer, .responsive_footer,
.footer_content, #footer_spacer, .footer_spacer, .valve_links,
.banner_open_in_steam, .home_gift_card_link {
  display: none !important;
}
.responsive_page_frame.with_header { padding-top: 0 !important; }
/* Dropdown de sugestões da busca da Steam: quem cuida disso agora é o
   StoreKeyboard do Arcadia (Big Picture). Usamos padrão amplo por atributo
   pra pegar todas as variantes que a Steam usa em templates diferentes
   (searchsuggest_container, searchsuggest_body_container, search_suggest_*,
   Popular searches box, etc.) sem quebrar em atualizações do site. */
[class*='searchsuggest'], [class*='search_suggest'],
[id*='search_suggestion'], [id*='searchsuggest'] {
  display: none !important;
}
`

function esconderCasca() {
  if (document.getElementById(CHROME_ID)) return
  const style = document.createElement("style")
  style.id = CHROME_ID
  style.textContent = CHROME_CSS
  ;(document.head || document.documentElement).appendChild(style)
}

// Porta o accent do tema do Arcadia para a loja: toque leve nos elementos de
// ação/destaque (botões de comprar, preços, abas ativas), sem recolorir o
// fundo/painéis — o fundo navy da Steam já combina e o recolor pesado ficava
// feio. Dirigido por --arc-accent (setado quando o host manda arcadia:tema).
const ACCENT_ID = "arcadia-accent"
const ACCENT_CSS = `
:root { --arc-accent: #00a8ff; }
/* Botões de ação verdes → accent do Arcadia */
.btn_green_steamui, .btn_addtocart, .btnv6_green_white_innerfade,
.game_purchase_action .btn_green_white_innerfade {
  background: var(--arc-accent) !important;
  color: #05121f !important;
}
/* Preço com desconto e destaques usam o accent */
.discount_final_price, .game_purchase_price.price { color: #ffffff !important; }
.discount_pct { color: #05121f !important; }
/* Aba ativa da sub-navegação da loja */
.home_tabs_row .tab.active, .tab_filter_control.checked {
  border-color: var(--arc-accent) !important;
}
/* Barra de ações do Arcadia: brilho no hover */
#arcadia-actionbar button:hover { filter: brightness(1.12); }
#arcadia-actionbar button:active { transform: translateY(1px); }
`

function aplicarAccent(accent) {
  let style = document.getElementById(ACCENT_ID)
  if (!style) {
    style = document.createElement("style")
    style.id = ACCENT_ID
    style.textContent = ACCENT_CSS
    ;(document.head || document.documentElement).appendChild(style)
  }
  if (accent) document.documentElement.style.setProperty("--arc-accent", accent)
}

// appid da página atual, ou "" se não for uma página de jogo (/app/<digits>).
function appidAtual() {
  const m = /\/app\/(\d+)/.exec(location.pathname)
  return m ? m[1] : ""
}

function tituloAtual() {
  const el = document.querySelector(".apphub_AppName")
  const t = (el && el.textContent) || document.title || ""
  return t.replace(/\s+on Steam\s*$/i, "").trim()
}

function acao(tipo) {
  ipcRenderer.sendToHost("arcadia:acao", { tipo, appid: appidAtual(), title: tituloAtual() })
}

function botao(rotulo, tipo, { primario = false, perigo = false, off = false } = {}) {
  const b = document.createElement("button")
  b.textContent = rotulo
  b.disabled = off
  b.style.cssText = [
    "appearance:none",
    "border:1px solid " + (primario ? "transparent" : perigo ? "rgba(255,107,129,.5)" : "rgba(255,255,255,.16)"),
    "cursor:" + (off ? "default" : "pointer"),
    "opacity:" + (off ? ".5" : "1"),
    "padding:8px 16px",
    "border-radius:9px",
    "font-size:12.5px",
    "font-weight:700",
    "line-height:1",
    "white-space:nowrap",
    "transition:filter .12s,transform .06s",
    "color:" + (primario ? "#04121f" : perigo ? "#ff6b81" : "#dbe7ff"),
    "background:" + (primario ? "var(--arc-accent,#00a8ff)" : perigo ? "transparent" : "rgba(30,38,52,.9)"),
  ].join(";")
  if (!off) b.addEventListener("click", (e) => { e.preventDefault(); acao(tipo) })
  return b
}

// (Re)desenha os botões conforme o estado atual, sem remexer na posição.
function montarBotoes(wrap) {
  wrap.innerHTML = ""
  wrap.appendChild(botao(estado.ocupado ? "…" : labels.baixar, "baixar", { primario: true, off: estado.ocupado }))
  if (estado.adicionado) {
    wrap.appendChild(botao(labels.remover, "remover", { perigo: true, off: estado.ocupado }))
  } else {
    wrap.appendChild(botao(labels.adicionar, "adicionar", { off: estado.ocupado }))
  }
  wrap.appendChild(botao(labels.restart, "restart", {}))
}

// Mostra os botões em página de jogo, INLINE à esquerda do "Community Hub"
// (dentro de .apphub_OtherSiteInfo). Se esse container não existir, cai numa
// barra fixa no topo direito como fallback. Remove fora de página de jogo.
function sync() {
  const emJogo = Boolean(appidAtual())
  const existente = document.getElementById(BAR_ID)
  if (!emJogo) {
    if (existente) existente.remove()
    return
  }
  // Já colocado E ainda no DOM (a Steam não recriou o cabeçalho): nada a fazer.
  if (existente && document.contains(existente)) return
  if (existente) existente.remove()
  if (!document.body) return

  const wrap = document.createElement("span")
  wrap.id = BAR_ID
  montarBotoes(wrap)
  // Avisa o host qual jogo está aberto, para ele devolver o estado (adicionado?).
  ipcRenderer.sendToHost("arcadia:pagina", { appid: appidAtual(), title: tituloAtual() })

  const host = document.querySelector(".apphub_OtherSiteInfo")
  if (host) {
    // Inline, antes do botão Community Hub (à esquerda dele).
    wrap.style.cssText = "display:inline-flex;align-items:center;gap:8px;margin-right:10px;vertical-align:middle;font-family:'Motiva Sans',Arial,sans-serif"
    host.insertBefore(wrap, host.firstChild)
  } else {
    // Fallback: barra fixa no topo direito.
    wrap.style.cssText = [
      "position:fixed", "top:0", "right:0", "display:inline-flex",
      "align-items:center", "gap:8px", "padding:10px 14px",
      "background:rgba(8,12,18,.92)", "backdrop-filter:blur(6px)",
      "border-left:1px solid rgba(255,255,255,.1)",
      "border-bottom:1px solid rgba(255,255,255,.1)",
      "border-radius:0 0 0 12px", "box-shadow:0 6px 20px rgba(0,0,0,.35)",
      "font-family:'Motiva Sans',Arial,sans-serif", "z-index:2147483000",
    ].join(";")
    document.body.appendChild(wrap)
  }
}

ipcRenderer.on("arcadia:labels", (_e, novo) => {
  labels = { ...labels, ...(novo || {}) }
  const wrap = document.getElementById(BAR_ID)
  if (wrap) montarBotoes(wrap)
})

ipcRenderer.on("arcadia:estado", (_e, novo) => {
  estado = { ...estado, ...(novo || {}) }
  const wrap = document.getElementById(BAR_ID)
  if (wrap) montarBotoes(wrap)
})

ipcRenderer.on("arcadia:tema", (_e, novo) => {
  aplicarAccent((novo && novo.accent) || "")
})

// Rola a página da Steam por comando do host (analógico direito do controle).
// Usar scrollBy no scrollingElement funciona tanto quando o body rola quanto
// quando a Steam usa um wrapper com overflow (algumas seções fazem isso).
ipcRenderer.on("arcadia:scroll", (_e, { dy = 0, dx = 0 } = {}) => {
  const alvo = document.scrollingElement || document.documentElement || document.body
  if (!alvo) return
  if (dy) alvo.scrollTop += dy
  if (dx) alvo.scrollLeft += dx
})

// Barra de busca da Steam: em Big Picture não faz sentido abrir o teclado do
// sistema (nem existe). Ao focar/clicar no campo, tira o foco IMEDIATAMENTE
// (evita teclado do SO / cursor piscando) e pede pro host abrir o StoreKeyboard,
// que devolve o texto pronto e o host navega direto pra /search/.
// Amplo pra cobrir templates diferentes da Steam (header clássico, novo
// responsivo, alguns temas de comunidade que reciclam a busca). Se qualquer
// um desses casar, tratamos como "a barra de busca da loja foi clicada".
const SEARCH_SELECTOR = [
  "#store_nav_search_term",
  "input[name='term']",
  "input[type='search']",
  "input[placeholder*='earch' i]",
  "input[placeholder*='uscar' i]",
  "input[placeholder*='oja' i]",
  "input[aria-label*='earch' i]",
  "[role='searchbox']",
  ".searchbox input",
  ".search_field",
  "#searchtext_box",
].join(", ")
// Candidatos do container do dropdown de sugestões. Testamos em ordem e
// ficamos com o primeiro que existir. Steam já mudou esses seletores antes;
// deixar mais de um evita quebrar em atualizações do site.
const SUGGEST_SELECTORS = [
  ".searchsuggest_body",
  ".searchsuggest_body_container",
  "#search_suggestion_contents",
  ".search_suggest",
  "#search_suggestion",
]

function acharCampoBusca() {
  return document.querySelector(SEARCH_SELECTOR)
}

function acharContainerSugestoes() {
  for (const sel of SUGGEST_SELECTORS) {
    const el = document.querySelector(sel)
    if (el) return el
  }
  return null
}

function pedirTecladoBusca() {
  const el = acharCampoBusca()
  const valor = (el && el.value) || ""
  // NÃO blurar: a Steam só gera sugestões enquanto o input está focado. O
  // teclado virtual vive no host (documento diferente), então esse foco aqui
  // dentro do webview não briga com nada do overlay.
  if (el) { try { el.focus() } catch {} }
  ipcRenderer.sendToHost("arcadia:pedirTeclado", { valor })
}

// ---------------------------------------------------------------------------
// Cursor virtual: o host lê o analógico esquerdo do controle e manda a posição
// pra cá; a gente desenha uma bolinha, dispara mouseover no elemento embaixo
// (abre os dropdowns "Browse ▾/Categories ▾" da Steam sem precisar clicar) e,
// no A (arcadia:clique), dispara mousedown/mouseup/click no elemento sob o
// cursor. O host é a autoridade sobre a posição — o preload não vê o gamepad
// direito porque a webview raramente tem foco de janela.
// ---------------------------------------------------------------------------
const CURSOR_ID = "arcadia-cursor"
const CURSOR_SIZE = 22
let cursorX = -9999
let cursorY = -9999
let ultimoHover = null

function garantirCursor() {
  let el = document.getElementById(CURSOR_ID)
  if (el) return el
  el = document.createElement("div")
  el.id = CURSOR_ID
  el.style.cssText = [
    "position:fixed",
    "top:0", "left:0",
    `width:${CURSOR_SIZE}px`, `height:${CURSOR_SIZE}px`,
    "border-radius:50%",
    "background:radial-gradient(circle at 35% 35%, #ffffff 0%, #dbe7ff 55%, rgba(0,168,255,.6) 100%)",
    "box-shadow:0 0 0 2px rgba(0,168,255,.55), 0 6px 18px rgba(0,0,0,.5)",
    "pointer-events:none",
    "z-index:2147483600",
    "transition:transform .04s linear, opacity .12s linear, background .12s linear",
    "opacity:0",
    "transform:translate3d(-9999px,-9999px,0)",
    "will-change:transform",
  ].join(";")
  ;(document.body || document.documentElement).appendChild(el)
  return el
}

function ehClicavel(el) {
  for (let cur = el; cur && cur !== document.documentElement; cur = cur.parentElement) {
    const tag = cur.tagName
    if (tag === "A" || tag === "BUTTON" || tag === "INPUT" || tag === "SELECT" || tag === "TEXTAREA") return true
    if (cur.getAttribute && cur.getAttribute("role") === "button") return true
    if (cur.onclick) return true
    const cursorCss = getComputedStyle(cur).cursor
    if (cursorCss === "pointer") return true
  }
  return false
}

function sinteticoMouse(tipo, alvo, x, y) {
  if (!alvo) return
  try {
    alvo.dispatchEvent(new MouseEvent(tipo, {
      bubbles: true, cancelable: true, view: window,
      clientX: x, clientY: y, button: 0, buttons: tipo === "mousedown" ? 1 : 0,
    }))
  } catch {}
}

function moverHover(x, y) {
  // pointer-events:none garante que elementFromPoint NÃO retorne o cursor.
  const alvo = document.elementFromPoint(x, y)
  if (alvo === ultimoHover) {
    sinteticoMouse("mousemove", alvo, x, y)
    return
  }
  if (ultimoHover) {
    sinteticoMouse("mouseout", ultimoHover, x, y)
    sinteticoMouse("mouseleave", ultimoHover, x, y)
  }
  ultimoHover = alvo
  if (alvo) {
    sinteticoMouse("mouseover", alvo, x, y)
    sinteticoMouse("mouseenter", alvo, x, y)
    sinteticoMouse("mousemove", alvo, x, y)
  }
  const el = document.getElementById(CURSOR_ID)
  if (el) {
    // Feedback visual: fica ligeiramente maior/mais brilhante sobre clicáveis.
    if (alvo && ehClicavel(alvo)) {
      el.style.background = "radial-gradient(circle at 35% 35%, #ffffff 0%, #ffffff 55%, var(--arc-accent,#00a8ff) 100%)"
    } else {
      el.style.background = "radial-gradient(circle at 35% 35%, #ffffff 0%, #dbe7ff 55%, rgba(0,168,255,.6) 100%)"
    }
  }
}

ipcRenderer.on("arcadia:cursor", (_e, { x = -9999, y = -9999 } = {}) => {
  cursorX = x; cursorY = y
  const el = garantirCursor()
  el.style.opacity = "1"
  // -CURSOR_SIZE/2 pra centralizar a bolinha em (x, y).
  el.style.transform = `translate3d(${x - CURSOR_SIZE / 2}px, ${y - CURSOR_SIZE / 2}px, 0)`
  moverHover(x, y)
})

ipcRenderer.on("arcadia:cursorVisivel", (_e, { v = true } = {}) => {
  const el = document.getElementById(CURSOR_ID)
  if (!el) return
  el.style.opacity = v ? "1" : "0"
  if (!v && ultimoHover) {
    sinteticoMouse("mouseout", ultimoHover, cursorX, cursorY)
    sinteticoMouse("mouseleave", ultimoHover, cursorX, cursorY)
    ultimoHover = null
  }
})

ipcRenderer.on("arcadia:clique", () => {
  const x = cursorX, y = cursorY
  const alvo = document.elementFromPoint(x, y)
  if (!alvo) return
  sinteticoMouse("mousedown", alvo, x, y)
  sinteticoMouse("mouseup", alvo, x, y)
  // .click() cobre casos onde a Steam usa onclick da propriedade (não listener)
  // e também links <a href> — o dispatch de "click" também funciona, mas .click()
  // segue o caminho nativo do elemento e navega no href sem depender de handler.
  try { alvo.click() } catch { sinteticoMouse("click", alvo, x, y) }
})

function ligarBuscaLoja() {
  // Delegation no document — o input pode ser recriado pela SPA da Steam.
  // Três porteiros: focusin (foco por teclado/tab), click (clique real ou do
  // cursor virtual) e mousedown (Steam foca no mousedown em algumas variantes,
  // então nosso click chegaria tarde). Basta um dos três casar.
  const acionar = (t) => {
    if (!t || !t.closest) return
    if (t.closest(SEARCH_SELECTOR)) pedirTecladoBusca()
  }
  document.addEventListener("focusin", (e) => {
    const t = e.target
    if (t && t.matches && t.matches(SEARCH_SELECTOR)) pedirTecladoBusca()
  }, true)
  document.addEventListener("mousedown", (e) => acionar(e.target), true)
  document.addEventListener("click", (e) => {
    if (e.target && e.target.closest && e.target.closest(SEARCH_SELECTOR)) {
      e.preventDefault()
    }
    acionar(e.target)
  }, true)
}

// ---------------------------------------------------------------------------
// Espelho das sugestões da busca.
//
// A Steam segue gerando o dropdown internamente (o input está focado); a gente
// só esconde ele visualmente (CSS acima) e observa o DOM pra extrair os itens.
// O host renderiza no visual do Arcadia dentro do teclado virtual. Se o seletor
// quebrar num update da Steam, cai pra sugestoes:[] e o teclado ainda serve
// pra ir pra /search/ (degradação graciosa, sem tela travada).
// ---------------------------------------------------------------------------
function absolutizarUrl(u) {
  if (!u) return ""
  try { return new URL(u, location.origin).href } catch { return u }
}

function extrairSugestoes(container) {
  if (!container) return []
  const out = []
  const vistos = new Set()
  const itens = container.querySelectorAll("a[href*='/app/']")
  for (const a of itens) {
    const m = /\/app\/(\d+)/.exec(a.getAttribute("href") || "")
    if (!m) continue
    const appid = m[1]
    if (vistos.has(appid)) continue
    vistos.add(appid)
    let titulo = ""
    const tEl = a.querySelector(".match_name, .match_app_name, .search_name")
    if (tEl) titulo = tEl.textContent || ""
    if (!titulo) titulo = (a.textContent || "").replace(/\s+/g, " ").trim()
    let preco = ""
    const pEl = a.querySelector(".match_price, .match_subtitle, .search_price")
    if (pEl) preco = (pEl.textContent || "").trim()
    // A /search/suggest devolve capsule_sm_120.jpg (120×45), pequena demais
    // pros cards grandes do teclado. header.jpg (460×215) é a mesma arte,
    // sempre disponível na CDN por appid, e nítida no card 184×92.
    out.push({
      appid,
      title: (titulo || "").trim(),
      preco: preco || "",
      img: `https://cdn.cloudflare.steamstatic.com/steam/apps/${appid}/header.jpg`,
    })
    if (out.length >= 8) break
  }
  return out
}

let sugestT = null
let sugestSeq = 0
// Consulta direta ao endpoint público de suggest da Steam. Estratégia mais
// robusta que triggar o jQuery deles: eles filtram eventos sintéticos por
// isTrusted, então setar input.value + dispatch não dispara a AJAX. Aqui
// pedimos o mesmo HTML que o dropdown deles renderiza e parseamos os itens.
async function buscarSugestoes(termo) {
  const meu = ++sugestSeq
  const q = String(termo || "").trim()
  if (!q) {
    ipcRenderer.sendToHost("arcadia:sugestoes", { items: [] })
    return
  }
  try {
    const url =
      `${location.origin}/search/suggest?` +
      `term=${encodeURIComponent(q)}&f=games&cc=BR&use_search_suggestions=1`
    const resp = await fetch(url, { credentials: "include" })
    if (meu !== sugestSeq) return // outra digitação em cima; joga fora
    const html = await resp.text()
    if (meu !== sugestSeq) return
    const box = document.createElement("div")
    box.innerHTML = html
    const items = extrairSugestoes(box)
    ipcRenderer.sendToHost("arcadia:sugestoes", { items })
  } catch {
    if (meu !== sugestSeq) return
    ipcRenderer.sendToHost("arcadia:sugestoes", { items: [] })
  }
}

function agendarSugestoes(termo) {
  if (sugestT) clearTimeout(sugestT)
  sugestT = setTimeout(() => {
    sugestT = null
    buscarSugestoes(termo)
  }, 180)
}

// Recebe cada tecla vinda do StoreKeyboard: mantém o value do input da Steam
// espelhando o que está no teclado (só pra ficar coerente se o user olhar) e
// dispara a busca direta no endpoint público de suggest — o autocomplete do
// jQuery da Steam filtra eventos sintéticos por isTrusted, então não dá pra
// contar com ele.
ipcRenderer.on("arcadia:tecla", (_e, { value = "" } = {}) => {
  const el = acharCampoBusca()
  if (el) {
    try { el.value = String(value) } catch {}
  }
  agendarSugestoes(String(value))
})

// Chamado quando o teclado fecha sem confirmar (B): limpa o campo pra não
// deixar termo "fantasma" da busca anterior no input da barra da Steam.
ipcRenderer.on("arcadia:limparBusca", () => {
  const el = acharCampoBusca()
  if (!el) return
  el.value = ""
  try { el.dispatchEvent(new Event("input", { bubbles: true })) } catch {}
})

function iniciar() {
  esconderCasca()
  aplicarAccent("")
  ligarBuscaLoja()
  sync()
  // A Steam navega/troca partes da página sem reload (SPA-ish). Reavalia a URL
  // e reinjeta/remove a barra conforme entra/sai de página de jogo, e
  // reinjeta o esconde-casca se a Steam recriar o header/footer.
  let t = null
  const obs = new MutationObserver(() => {
    if (t) return
    t = setTimeout(() => {
      t = null
      if (!document.getElementById(CHROME_ID)) esconderCasca()
      if (!document.getElementById(ACCENT_ID)) aplicarAccent("")
      sync()
    }, 400)
  })
  try {
    obs.observe(document.body, { childList: true, subtree: true })
  } catch {}
}

if (document.readyState === "loading") {
  document.addEventListener("DOMContentLoaded", iniciar)
} else {
  iniciar()
}
