// Atualização do app EMPACOTADO (AppImage/NSIS), via electron-updater.
//
// O updater git (./updater.js) continua sendo o canal de quem roda da fonte.
// Este é o segundo canal: pergunta antes de baixar (autoDownload=false),
// instala só no "Reiniciar agora" (autoInstallOnAppQuit=false) e nunca deixa
// o EventEmitter do autoUpdater derrubar o processo (listener de `error`).
//
// A suíte roda sem Electron: tudo que toca o Electron entra por injeção.

const CANAIS_SUPORTADOS = new Set(["nsis", "appimage"])

/** Compara versões ("v1.4.10" > "1.4.9" — numérico, não lexicográfico). */
function compararVersao(a, b) {
  const partes = (v) =>
    String(v || "")
      .replace(/^v/i, "")
      .split(".")
      .map((n) => parseInt(n, 10) || 0)
  const pa = partes(a)
  const pb = partes(b)
  for (let i = 0; i < Math.max(pa.length, pb.length); i++) {
    const d = (pa[i] || 0) - (pb[i] || 0)
    if (d !== 0) return d > 0 ? 1 : -1
  }
  return 0
}

/** Bytes do arquivo oferecido: o yml publica `files[].size`. */
function tamanhoDe(info) {
  const arquivos = Array.isArray(info?.files) ? info.files : []
  let total = 0
  for (const f of arquivos) {
    const n = Number(f?.size)
    if (Number.isFinite(n) && n > 0) total += n
  }
  return total > 0 ? total : null
}

/**
 * Canal do app empacotado, POR PLATAFORMA (B4):
 * - Linux: `APPIMAGE` é o que define AppImage; sem ela o pacote foi extraído
 *   ou movido e não há como se substituir → `sem_suporte` (aviso com link),
 *   NUNCA `nsis` (o Linux sempre escreve app-update.yml).
 * - Windows: portable pelo env do stub; `zip` quando falta o app-update.yml;
 *   `nsis` quando ele existe.
 * - `fonte`: não empacotado.
 */
function detectarCanal({ app, env, temAppUpdateYml, platform }) {
  if (!app.isPackaged) return "fonte"
  if (platform === "linux") return env.APPIMAGE ? "appimage" : "sem_suporte"
  if (platform === "win32") {
    if (env.PORTABLE_EXECUTABLE_FILE || env.PORTABLE_EXECUTABLE_DIR) return "portable"
    return temAppUpdateYml ? "nsis" : "zip"
  }
  return "sem_suporte"
}

/**
 * Contrato único (B3):
 *   createPackagedUpdater({
 *     app, autoUpdater, env, temAppUpdateYml, platform,
 *     isJogoRodando, jaAvisado, salvarJaAvisado, pendente, salvarPendente, onChange,
 *   }) -> { canal(), suportado(), estado(), checar(), baixar(), instalar(), marcarJaAvisado() }
 *
 * estado(): { canal, suportado, versaoAtual, versaoNova, tamanho, fase,
 *             progresso, erro, erroDeFundo, erroAcao, jaAvisado, jogoRodando }
 * fases: ocioso | disponivel | baixando | pronto | erro | sem_suporte
 * checar({ manual }) -> { ok, disponivel, versao?, tamanho?, motivo?, erro? }
 * baixar() -> { ok, erro? }   instalar() -> { ok, erro? }
 *
 * `onChange(estado)` é chamado a cada mudança; o main só repassa para a janela.
 */
function createPackagedUpdater({
  app,
  autoUpdater = null,
  env = {},
  temAppUpdateYml = false,
  platform = process.platform,
  isJogoRodando = () => false,
  jaAvisado = () => false,
  salvarJaAvisado = () => {},
  pendente = () => null,
  salvarPendente = () => {},
  onChange = () => {},
}) {
  const canal = detectarCanal({ app, env, temAppUpdateYml, platform })
  const suportado = CANAIS_SUPORTADOS.has(canal)

  const estadoAtual = {
    canal,
    suportado,
    versaoAtual: String(app?.getVersion?.() || ""),
    versaoNova: null,
    tamanho: null,
    // `fonte` não é "sem suporte": o canal git cuida dele. `sem_suporte` é o
    // aviso com link do portable/zip/AppImage extraído.
    fase: suportado || canal === "fonte" ? "ocioso" : "sem_suporte",
    progresso: 0,
    erro: null,
    erroDeFundo: false,
    erroAcao: null,
    jaAvisado: false,
    jogoRodando: false,
  }

  // Separa erro de fundo (checagem automática → silêncio) de erro de ação do
  // usuário (checagem manual/baixar → diálogo). `erroAcao` diz ao diálogo qual
  // retry oferecer (N8): "checar" → Tentar de novo; "baixar" → Baixar;
  // "instalar" → Tentar de novo (o check revalida o cache e devolve "pronto").
  let acaoUsuario = null

  const snapshot = () => ({ ...estadoAtual, jogoRodando: Boolean(isJogoRodando()) })
  const publicar = () => {
    try {
      onChange(snapshot())
    } catch {}
  }
  const mudar = (patch) => {
    Object.assign(estadoAtual, patch)
    publicar()
  }
  const mensagem = (e) => String(e?.message || e || "erro")

  if (suportado && autoUpdater) {
    autoUpdater.autoDownload = false
    autoUpdater.autoInstallOnAppQuit = false

    autoUpdater.on("update-available", (info) => {
      const versao = String(info?.version || "")
      mudar({
        fase: "disponivel",
        versaoNova: versao,
        tamanho: tamanhoDe(info),
        progresso: 0,
        erro: null,
        erroDeFundo: false,
        erroAcao: null,
        jaAvisado: Boolean(jaAvisado(versao)),
      })
    })

    autoUpdater.on("download-progress", (p) => {
      mudar({
        fase: "baixando",
        progresso: Math.max(0, Math.min(100, Math.round(Number(p?.percent) || 0))),
        erro: null,
        erroAcao: null,
      })
    })

    autoUpdater.on("update-downloaded", (info) => {
      const versao = String(info?.version || estadoAtual.versaoNova || "")
      if (versao) {
        try {
          salvarPendente(versao)
        } catch {}
      }
      mudar({
        fase: "pronto",
        versaoNova: versao || estadoAtual.versaoNova,
        progresso: 100,
        erro: null,
        erroDeFundo: false,
        erroAcao: null,
      })
    })

    autoUpdater.on("update-not-available", () => {
      try {
        salvarPendente(null)
      } catch {}
      mudar({
        fase: "ocioso",
        versaoNova: null,
        tamanho: null,
        progresso: 0,
        erro: null,
        erroDeFundo: false,
        erroAcao: null,
        jaAvisado: false,
      })
    })

    // Sem este listener o EventEmitter lança e pode derrubar o processo. O
    // handler NUNCA re-lança: só registra o estado (erro de fundo não abre UI).
    autoUpdater.on("error", (e) => {
      mudar({ fase: "erro", erro: mensagem(e), erroDeFundo: !acaoUsuario, erroAcao: acaoUsuario })
    })
  }

  async function checar({ manual = false } = {}) {
    if (!suportado) {
      mudar({ fase: "sem_suporte" })
      return { ok: false, disponivel: false, motivo: "canal_nao_suportado" }
    }
    // Ciclo automático não interrompe partida; reavalia no próximo ciclo.
    if (!manual && isJogoRodando()) {
      mudar({ jogoRodando: true })
      return { ok: true, disponivel: false, motivo: "jogo_rodando" }
    }

    acaoUsuario = manual ? "checar" : null
    try {
      const r = await autoUpdater.checkForUpdates()
      const info = r?.updateInfo || null
      const versao = String(info?.version || "")
      const disponivel = Boolean(versao) && compararVersao(versao, estadoAtual.versaoAtual) > 0

      if (!disponivel) {
        mudar({
          fase: "ocioso",
          versaoNova: null,
          tamanho: null,
          progresso: 0,
          erro: null,
          erroDeFundo: false,
          erroAcao: null,
          jaAvisado: false,
        })
        return { ok: true, disponivel: false, versao: null }
      }

      // D6: já baixado numa sessão anterior. O electron-updater valida o cache
      // por sha512 e reemite `update-downloaded` sem baixar de novo; se o cache
      // sumiu, baixa a MESMA versão que o usuário já aceitou.
      if (pendente() === versao) {
        mudar({ fase: "baixando", progresso: 0, erro: null, erroDeFundo: false, erroAcao: null })
        await autoUpdater.downloadUpdate()
        if (estadoAtual.fase !== "pronto") mudar({ fase: "pronto", progresso: 100 })
        return { ok: true, disponivel: true, versao, tamanho: tamanhoDe(info) }
      }

      if (manual) {
        // Checagem manual mostra mesmo com "já avisei" — foi o usuário quem pediu.
        mudar({
          fase: "disponivel",
          versaoNova: versao,
          tamanho: tamanhoDe(info),
          erro: null,
          erroAcao: null,
          jaAvisado: false,
        })
      } else if (estadoAtual.fase !== "disponivel" || estadoAtual.versaoNova !== versao) {
        mudar({
          fase: "disponivel",
          versaoNova: versao,
          tamanho: tamanhoDe(info),
          erro: null,
          erroAcao: null,
          jaAvisado: Boolean(jaAvisado(versao)),
        })
      }
      return { ok: true, disponivel: true, versao, tamanho: tamanhoDe(info) }
    } catch (e) {
      mudar({
        fase: "erro",
        erro: mensagem(e),
        erroDeFundo: !manual,
        erroAcao: manual ? "checar" : null,
      })
      return { ok: false, disponivel: false, erro: mensagem(e), motivo: "erro" }
    } finally {
      acaoUsuario = null
    }
  }

  async function baixar() {
    if (!suportado) return { ok: false, erro: "canal_nao_suportado" }
    if (!estadoAtual.versaoNova) return { ok: false, erro: "sem_versao" }
    acaoUsuario = "baixar"
    mudar({ fase: "baixando", progresso: 0, erro: null, erroDeFundo: false, erroAcao: null })
    try {
      await autoUpdater.downloadUpdate()
      if (estadoAtual.fase !== "pronto") mudar({ fase: "pronto", progresso: 100 })
      return { ok: true }
    } catch (e) {
      mudar({ fase: "erro", erro: mensagem(e), erroDeFundo: false, erroAcao: "baixar" })
      return { ok: false, erro: mensagem(e) }
    } finally {
      acaoUsuario = null
    }
  }

  function instalar() {
    if (!suportado) return { ok: false, erro: "canal_nao_suportado" }
    if (estadoAtual.fase !== "pronto") return { ok: false, erro: "sem_download" }
    try {
      autoUpdater.quitAndInstall()
      return { ok: true }
    } catch (e) {
      mudar({ fase: "erro", erro: mensagem(e), erroDeFundo: false, erroAcao: "instalar" })
      return { ok: false, erro: mensagem(e) }
    }
  }

  function marcarJaAvisado(versao) {
    const v = String(versao || estadoAtual.versaoNova || "")
    if (!v) return { ok: false }
    try {
      salvarJaAvisado(v)
    } catch {}
    if (estadoAtual.versaoNova === v) mudar({ jaAvisado: true })
    return { ok: true }
  }

  return {
    canal: () => canal,
    suportado: () => suportado,
    estado: () => snapshot(),
    checar,
    baixar,
    instalar,
    marcarJaAvisado,
  }
}

module.exports = { createPackagedUpdater, compararVersao, tamanhoDe, detectarCanal }
