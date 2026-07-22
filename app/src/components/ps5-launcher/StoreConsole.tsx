"use client"

import { forwardRef, useEffect, useRef, useState } from "react"
import type { Game } from "./types"
import { ConsoleDestinoDialog } from "./ConsoleDestinoDialog"
import { StoreKeyboard, type SugestaoLoja } from "./StoreKeyboard"
import { useStoreActions } from "../useStoreActions"
import { useI18n } from "../../i18n/I18nContext"

// i18n do Arcadia → parâmetro `l` da loja Steam.
const STEAM_LANG: Record<string, string> = {
  "pt-BR": "brazilian",
  "en-US": "english",
  "es-ES": "spanish",
}

interface StoreConsoleProps {
  games: Game[]
  /**
   * A loja está visível e em primeiro plano. Falso quando a janela perde o
   * foco OU quando o launcher está em outra aba — a loja continua montada
   * (para não recarregar a página da Steam), só escondida; os laços de
   * gamepad precisam parar nesse estado.
   */
  ativo: boolean
  /** A loja abriu/fechou um overlay próprio (só o diálogo de escolha de disco). */
  onOverlay?: (aberto: boolean) => void
  /** Atalhos do laço de gamepad do PS5Launcher (aqui só o B/voltar é útil). */
  onAtalhos?: (a: {
    baixar: (appid: string) => void
    adicionar: (appid: string) => void
    /** B: volta no histórico do webview. true se consumiu. */
    voltar: () => boolean
    /** Y: abre o teclado virtual direto (sem precisar mover o cursor até a barra). */
    abrirTeclado: () => void
  }) => void
}

// A loja do Arcadia é a loja web da Steam embutida num <webview>. A navegação
// e a abertura de jogos acontecem dentro dela; um preload
// (webview-steam-preload.js) injeta um botão discreto "Baixar (Arcadia)" nas
// páginas de jogo, que dispara o fluxo de download do próprio Arcadia.
export const StoreConsole = forwardRef<HTMLDivElement, StoreConsoleProps>(function StoreConsole(
  { games, ativo, onOverlay, onAtalhos },
  ref,
) {
  const { t, lang } = useI18n()
  const acoes = useStoreActions(games)
  const webRef = useRef<any>(null)
  // Histórico próprio da webview. O canGoBack() da <webview> mente com a SPA
  // da Steam (pushState, location.replace, popups negados que não navegam),
  // então mantemos nossa pilha alimentada por did-navigate + did-navigate-in-page.
  // voltandoRef desliga o push quando somos NÓS que estamos voltando (senão o
  // did-navigate da própria volta empilharia a URL de volta).
  const historicoRef = useRef<string[]>([])
  const voltandoRef = useRef(false)
  // Teclado virtual: aberto quando a barra de busca da Steam é clicada
  // (o preload avisa via IPC e a gente sobe o overlay do Arcadia).
  const [tecladoAberto, setTecladoAberto] = useState(false)
  const [tecladoValor, setTecladoValor] = useState("")
  // Sugestões espelhadas do dropdown da Steam. Preload observa o DOM interno
  // e nos manda a lista pronta; renderizamos como tira dentro do teclado.
  const [sugestoesLoja, setSugestoesLoja] = useState<SugestaoLoja[]>([])
  // A loja da Steam é uma página web de verdade: leva alguns segundos até
  // pintar. Sem isto a aba ficava preta e parecia travada.
  const [carregando, setCarregando] = useState(true)
  // appid do jogo aberto no webview + refs de estado (para o listener, que é
  // registrado uma vez, ler sempre o valor atual sem fechar sobre valor velho).
  const paginaAppidRef = useRef("")
  // Timestamp da última vez que o teclado fechou. Serve pra ignorar
  // arcadia:pedirTeclado disparado pelo focusin da barra de busca da Steam,
  // que algumas páginas de jogo auto-focam ao carregar depois do loadURL —
  // sem isso, o teclado reabria sozinho na cara do usuário logo depois de
  // escolher uma sugestão.
  const tecladoFechouEmRef = useRef(0)
  const jaAdRef = useRef(acoes.jaAdicionados)
  const busyRef = useRef(acoes.busy)
  jaAdRef.current = acoes.jaAdicionados
  busyRef.current = acoes.busy

  // Manda para a barra injetada se o jogo aberto já está adicionado + se está
  // ocupado, para ela mostrar Remover/estado certo.
  function enviarEstado(appid: string) {
    if (!appid) return
    try {
      webRef.current?.send("arcadia:estado", {
        adicionado: jaAdRef.current.has(appid),
        ocupado: Boolean(busyRef.current),
      })
    } catch {}
  }

  // Reenvia o estado quando "adicionados"/ocupado mudam (ex.: acabou de
  // adicionar → a barra troca Adicionar por Remover na hora).
  useEffect(() => {
    enviarEstado(paginaAppidRef.current)
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [acoes.jaAdicionados, acoes.busy])

  // Avisa o launcher quando algum overlay do próprio StoreConsole abre/fecha:
  // hoje, o diálogo de escolha de disco e o teclado virtual. Enquanto qualquer
  // um deles estiver aberto, o laço de gamepad da loja pausa (o overlay tem o
  // próprio) e o B da loja não sai da aba.
  useEffect(() => {
    onOverlay?.(Boolean(acoes.escolhendo) || tecladoAberto)
  }, [acoes.escolhendo, tecladoAberto, onOverlay])

  // Registra os atalhos de gamepad: aqui só o B (voltar no histórico) faz
  // sentido — a página é navegada por mouse, não pelo direcional.
  useEffect(() => {
    onAtalhos?.({
      baixar: () => {},
      adicionar: () => {},
      voltar: () => {
        // Consome a nossa pilha: se tem mais de uma entrada, tira a atual e
        // carrega a anterior. Se sobrou só a raiz (ou vazio), devolve false
        // pro launcher sair da aba loja pra Jogos, que é o comportamento certo
        // "não tem mais pra onde voltar".
        //
        // Coalescência EXTRA por assinatura da página: /app/1091500/ e
        // /app/1091500/Cyberpunk_2077/ têm pathnames diferentes mas são a
        // MESMA página (Steam redireciona a primeira pra segunda no load).
        // Se não pular essas duplicatas, o B carrega a versão "sem slug"
        // e a Steam reencaminha pra "com slug" — usuário vê reload.
        const assinatura = (u: string) => {
          try {
            const p = new URL(u).pathname.replace(/\/+$/, "")
            const mApp = /^\/app\/(\d+)/.exec(p)
            if (mApp) return "app:" + mApp[1]
            const mSub = /^\/sub\/(\d+)/.exec(p)
            if (mSub) return "sub:" + mSub[1]
            const mBundle = /^\/bundle\/(\d+)/.exec(p)
            if (mBundle) return "bundle:" + mBundle[1]
            return p || "/"
          } catch { return u }
        }
        const w = webRef.current
        const stack = historicoRef.current
        if (!w || stack.length < 2) return false
        const atualSig = assinatura(stack[stack.length - 1])
        stack.pop() // atual
        // Continua descendo enquanto as entradas restantes forem "a mesma
        // página" que a atual — só para quando achar uma REALMENTE diferente.
        while (stack.length >= 1 && assinatura(stack[stack.length - 1]) === atualSig) {
          stack.pop()
        }
        if (stack.length < 1) return false
        const anterior = stack[stack.length - 1]
        voltandoRef.current = true
        try { w.loadURL(anterior) } catch { voltandoRef.current = false; return false }
        return true
      },
      abrirTeclado: () => {
        setTecladoValor("")
        setTecladoAberto(true)
      },
    })
  }, [onAtalhos])

  // Eventos do webview: recebe as ações da barra injetada e dispara o hook.
  useEffect(() => {
    const el = webRef.current
    if (!el) return
    const onReady = () => {
      try {
        el.send("arcadia:labels", {
          baixar: t("store.baixar"),
          adicionar: t("store.adicionar_steam"),
          remover: t("common.remover"),
          restart: t("desktop.restart_steam"),
        })
        // Porta o accent do tema do Arcadia para a loja Steam (botões/destaques).
        const accent = getComputedStyle(document.documentElement)
          .getPropertyValue("--accent").trim() || "#00a8ff"
        el.send("arcadia:tema", { accent })
      } catch {}
    }
    const onMsg = (e: any) => {
      const arg = e?.args?.[0] || {}
      // A barra avisa qual jogo abriu → devolvemos o estado dele.
      if (e?.channel === "arcadia:pagina") {
        paginaAppidRef.current = String(arg.appid || "")
        enviarEstado(paginaAppidRef.current)
        return
      }
      // A barra de busca da Steam foi clicada → abrimos o teclado virtual.
      // Silêncio de 700ms depois do fechamento: cobre o auto-focus da barra
      // que acontece quando a página do jogo termina de carregar (loadURL de
      // uma sugestão escolhida), evitando o teclado reabrir sozinho.
      if (e?.channel === "arcadia:pedirTeclado") {
        if (Date.now() - tecladoFechouEmRef.current < 700) return
        setTecladoValor(String(arg.valor || ""))
        setTecladoAberto(true)
        return
      }
      // Sugestões vindas do preload (extraídas do dropdown nativo da Steam,
      // que está escondido). Normalizamos e ignoramos itens sem título.
      if (e?.channel === "arcadia:sugestoes") {
        const raw = Array.isArray(arg.items) ? arg.items : []
        const items: SugestaoLoja[] = raw
          .filter((x: any) => x && x.appid && x.title)
          .map((x: any) => ({
            appid: String(x.appid),
            title: String(x.title),
            preco: x.preco ? String(x.preco) : "",
            img: x.img ? String(x.img) : "",
          }))
        setSugestoesLoja(items)
        return
      }
      if (e?.channel !== "arcadia:acao") return
      const { tipo, appid, title } = arg
      if (tipo === "restart") { acoes.reiniciarSteam(); return }
      if (!appid) return
      const jogo = { appid: String(appid), title: String(title || appid) }
      if (tipo === "baixar") acoes.baixar(jogo)
      else if (tipo === "adicionar") acoes.adicionar(jogo)
      else if (tipo === "remover") acoes.remover(jogo)
    }
    // Alimenta o histórico próprio. did-navigate cobre navegações completas;
    // did-navigate-in-page cobre pushState/hash (SPA da Steam). Se somos NÓS
    // que estamos voltando (loadURL da URL anterior), pulamos o push — senão a
    // volta empilharia a URL de volta e o B deixaria de funcionar na próxima.
    //
    // Coalescência POR PATHNAME: a Steam empilha várias entradas para o que o
    // usuário vê como uma página só (pushState inicial + normalização de
    // query/tracking). Se o pathname já é o do topo, apenas atualiza a URL no
    // topo — assim um B corresponde a uma volta real, não à undo de uma query.
    const pathnameDe = (u: string) => {
      try { return new URL(u).pathname.replace(/\/+$/, "") || "/" } catch { return u }
    }
    const empilhar = (novaUrl: string) => {
      if (voltandoRef.current) { voltandoRef.current = false; return }
      const stack = historicoRef.current
      const topo = stack[stack.length - 1]
      if (topo === novaUrl) return
      if (topo && pathnameDe(topo) === pathnameDe(novaUrl)) {
        stack[stack.length - 1] = novaUrl
        return
      }
      stack.push(novaUrl)
      // Não deixa a pilha crescer sem limite em sessões longas.
      if (stack.length > 60) stack.splice(0, stack.length - 60)
    }
    const onNavigate = (e: any) => { if (e?.url) empilhar(String(e.url)) }
    const onNavigateInPage = (e: any) => {
      if (e?.isMainFrame === false) return
      if (e?.url) empilhar(String(e.url))
    }
    const onStart = () => setCarregando(true)
    const onStop = () => setCarregando(false)
    el.addEventListener("did-start-loading", onStart)
    el.addEventListener("did-stop-loading", onStop)
    el.addEventListener("dom-ready", onReady)
    el.addEventListener("ipc-message", onMsg)
    el.addEventListener("did-navigate", onNavigate)
    el.addEventListener("did-navigate-in-page", onNavigateInPage)
    return () => {
      el.removeEventListener("did-start-loading", onStart)
      el.removeEventListener("did-stop-loading", onStop)
      el.removeEventListener("dom-ready", onReady)
      el.removeEventListener("ipc-message", onMsg)
      el.removeEventListener("did-navigate", onNavigate)
      el.removeEventListener("did-navigate-in-page", onNavigateInPage)
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [])

  const l = STEAM_LANG[lang]
  const url = `https://store.steampowered.com/?cc=br${l ? `&l=${l}` : ""}`

  // Analógico direito rola a página da Steam por dentro. Mesma física do
  // GameOverview: repouso calibrado (permite gatilho analógico separado do
  // stick), deadzone, resposta quadrática e inércia. O scroll acontece no
  // preload — mandamos só o delta de pixels a cada frame. Pausa quando o
  // teclado ou o diálogo de disco estão abertos.
  useEffect(() => {
    let raf = 0
    let rest: number[] | null = null
    let vel = 0
    const loop = () => {
      const pausado = !ativo || tecladoAberto || Boolean(acoes.escolhendo)
      if (pausado || !document.hasFocus()) {
        vel = 0
        raf = requestAnimationFrame(loop)
        return
      }
      const pads = navigator.getGamepads ? navigator.getGamepads() : []
      const gp = Array.from(pads).find((p) => p) || null
      if (gp) {
        if (!rest) rest = Array.from(gp.axes)
        let ry = 0
        for (let ai = 2; ai < gp.axes.length; ai++) {
          const v = (gp.axes[ai] ?? 0) - (rest[ai] ?? 0)
          if (Math.abs(v) > Math.abs(ry)) ry = v
        }
        const target = Math.abs(ry) > 0.15 ? Math.sign(ry) * ry * ry * 46 : 0
        vel += (target - vel) * 0.25
        if (Math.abs(vel) > 0.5) {
          try { webRef.current?.send("arcadia:scroll", { dy: vel }) } catch {}
        }
      }
      raf = requestAnimationFrame(loop)
    }
    raf = requestAnimationFrame(loop)
    return () => cancelAnimationFrame(raf)
  }, [ativo, tecladoAberto, acoes.escolhendo])

  // Cursor virtual: analógico ESQUERDO move a bolinha desenhada pelo preload
  // dentro do webview. Botão A dispara clique no elemento embaixo do cursor.
  // O host é a autoridade sobre a posição — a webview raramente tem foco de
  // janela, então o Gamepad API não é confiável de dentro dela.
  useEffect(() => {
    let raf = 0
    let rest: number[] | null = null
    let x = 0, y = 0            // posição atual do cursor, coords do webview
    let iniciada = false        // já centralizei ao aparecer no controle?
    let prevA = false           // edge do botão A
    let ultimoEnvio = 0
    const MAX_PX_S = 900        // velocidade máxima ~= tela em ~1.5s

    const loop = (agora: number) => {
      const w = webRef.current
      const pausado = !ativo || tecladoAberto || Boolean(acoes.escolhendo)
      // Mesmo em pausa, sincronizamos prevA com o estado real do botão. Sem
      // isso, se o usuário aperta A pra escolher uma sugestão (o que fecha o
      // teclado), no frame seguinte o A ainda pode estar pressionado e o loop
      // interpreta como uma edge nova — dispararia arcadia:clique na barra de
      // busca da posição antiga do cursor, reabrindo o teclado sozinho.
      if (!w || pausado || !document.hasFocus()) {
        const padsP = navigator.getGamepads ? navigator.getGamepads() : []
        const gpP = Array.from(padsP).find((p) => p) || null
        if (gpP) prevA = Boolean(gpP.buttons[0]?.pressed)
        raf = requestAnimationFrame(loop)
        return
      }
      const pads = navigator.getGamepads ? navigator.getGamepads() : []
      const gp = Array.from(pads).find((p) => p) || null
      if (!gp) { raf = requestAnimationFrame(loop); return }
      if (!rest) rest = Array.from(gp.axes)

      const rect = (w.getBoundingClientRect?.() as DOMRect | undefined)
      const W = rect?.width || window.innerWidth
      const H = rect?.height || window.innerHeight
      if (!iniciada) { x = W / 2; y = H / 2; iniciada = true }

      // Mesma física do scroll: repouso calibrado, deadzone, quadrática.
      const lx = (gp.axes[0] ?? 0) - (rest[0] ?? 0)
      const ly = (gp.axes[1] ?? 0) - (rest[1] ?? 0)
      const vel = (v: number) => (Math.abs(v) > 0.15 ? Math.sign(v) * v * v : 0)
      // agora - ultimoEnvio ≈ 16.6ms; convertemos velocidade px/s em px/frame.
      const dt = ultimoEnvio ? Math.min(0.05, (agora - ultimoEnvio) / 1000) : 1 / 60
      const dx = vel(lx) * MAX_PX_S * dt
      const dy = vel(ly) * MAX_PX_S * dt

      if (dx || dy) {
        x = Math.max(0, Math.min(W, x + dx))
        y = Math.max(0, Math.min(H, y + dy))
        try { w.send("arcadia:cursor", { x, y }) } catch {}
      }
      ultimoEnvio = agora

      // Botão A (edge). Não chamamos activeElement.click aqui — quem clica é
      // o preload, no elemento embaixo do cursor. useGamepadNav do host está
      // com noFocusMove, então não vai duplicar.
      const a = Boolean(gp.buttons[0]?.pressed)
      if (a && !prevA) {
        try { w.send("arcadia:clique") } catch {}
      }
      prevA = a

      raf = requestAnimationFrame(loop)
    }
    raf = requestAnimationFrame(loop)
    return () => cancelAnimationFrame(raf)
  }, [ativo, tecladoAberto, acoes.escolhendo])

  // Esconde o cursor no preload sempre que um overlay do host cobrir a loja
  // (teclado, diálogo de disco). Mostra de novo quando fecha.
  useEffect(() => {
    const w = webRef.current
    if (!w) return
    const v = !(tecladoAberto || Boolean(acoes.escolhendo))
    try { w.send("arcadia:cursorVisivel", { v }) } catch {}
  }, [tecladoAberto, acoes.escolhendo])

  return (
    <div ref={ref} className="relative h-full w-full bg-black">
      <webview
        ref={webRef}
        src={url}
        partition="persist:steamstore"
        allowpopups={true}
        useragent="Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/122.0.0.0 Safari/537.36"
        style={{ width: "100%", height: "100%", background: "#000" }}
      />

      {/* Carregando: some sozinho no did-stop-loading. */}
      {carregando && (
        <div className="pointer-events-none absolute inset-0 z-[60] flex flex-col items-center justify-center gap-4 bg-black">
          <div
            className="h-10 w-10 animate-spin rounded-full border-2 border-white/15"
            style={{ borderTopColor: "var(--accent)" }}
          />
          <span className="text-sm tracking-wide text-white/40">{t("store.titulo")}</span>
        </div>
      )}

      {/* Escolha da biblioteca Steam — mesmo diálogo do hook de ações. */}
      {acoes.escolhendo && (
        <ConsoleDestinoDialog
          titulo={t("ps5.instalar.titulo", { title: acoes.escolhendo.jogo.title })}
          subtitulo={t("ps5.steam_lib.subtitulo")}
          opcoes={acoes.escolhendo.libs.map((l) => ({
            caminho: l.steamDir,
            rotulo: t("ps5.steam_lib.opcao"),
            livre: l.free,
          }))}
          onEscolher={(steamDir) =>
            acoes.escolhendo && acoes.confirmarBaixar(acoes.escolhendo.jogo, acoes.escolhendo.info, steamDir)
          }
          onFechar={() => acoes.setEscolhendo(null)}
        />
      )}

      <StoreKeyboard
        aberto={tecladoAberto}
        inicial={tecladoValor}
        sugestoes={sugestoesLoja}
        onTexto={(v) => {
          // Toda tecla vai pra Steam via IPC; o autocomplete deles roda no
          // input focado e a lista volta pela porta arcadia:sugestoes.
          try { webRef.current?.send("arcadia:tecla", { value: v }) } catch {}
        }}
        onEscolherSugestao={(appid) => {
          tecladoFechouEmRef.current = Date.now()
          setTecladoAberto(false)
          setSugestoesLoja([])
          const alvo =
            `https://store.steampowered.com/app/${encodeURIComponent(appid)}/` +
            `?cc=br${l ? `&l=${l}` : ""}`
          try { webRef.current?.loadURL(alvo) } catch {}
        }}
        onFechar={() => {
          tecladoFechouEmRef.current = Date.now()
          setTecladoAberto(false)
          setSugestoesLoja([])
          // Limpa o campo dentro do webview pra não ficar termo fantasma da
          // busca anterior aparecendo se abrir de novo.
          try { webRef.current?.send("arcadia:limparBusca") } catch {}
        }}
        onConfirmar={(texto) => {
          tecladoFechouEmRef.current = Date.now()
          setTecladoAberto(false)
          setSugestoesLoja([])
          const termo = texto.trim()
          if (!termo) return
          const alvo =
            `https://store.steampowered.com/search/?term=${encodeURIComponent(termo)}` +
            `&cc=br${l ? `&l=${l}` : ""}`
          try { webRef.current?.loadURL(alvo) } catch {}
        }}
      />

      {acoes.toast && (
        <div
          className="fixed bottom-8 right-8 z-[95] max-w-[420px] rounded-xl border border-white/15 bg-[#0d1017]/95 px-5 py-4 text-sm text-white/90 shadow-2xl backdrop-blur-md"
          onClick={() => acoes.setToast("")}
        >
          {acoes.toast}
        </div>
      )}
    </div>
  )
})
