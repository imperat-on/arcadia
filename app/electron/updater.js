// Atualização do próprio Arcadia, pelo Git.
//
// O app não é empacotado: o install.sh clona o repositório em
// ~/.local/share/arcadia e o arcadia.sh reconstrói o front-end quando algum
// fonte fica mais novo que o dist/. Então "atualizar" é `git pull` + rebuild —
// não há instalador nem AppImage envolvidos.
//
// Descobrir que há novidade não exige git nenhum: a API pública do GitHub
// compara dois commits e devolve, numa chamada só, quantos faltam, as
// mensagens deles e a lista de arquivos alterados.

const fs = require("fs")
const path = require("path")
const { execFile } = require("child_process")
const { fetchRede } = require("./httpfetch")

const RAIZ = path.resolve(__dirname, "../..")
const APP = path.join(RAIZ, "app")
const REPO = "imperat-on/arcadia"
const BRANCH = "master"
const API = `https://api.github.com/repos/${REPO}`

/** Roda um comando e devolve { ok, saida, erro }, sem nunca lançar. */
function run(cmd, args, opts = {}) {
  return new Promise((resolve) => {
    execFile(
      cmd,
      args,
      { cwd: RAIZ, timeout: 10 * 60 * 1000, maxBuffer: 8 * 1024 * 1024, ...opts },
      (e, stdout, stderr) => {
        resolve({
          ok: !e,
          saida: String(stdout || "").trim(),
          erro: e ? String(stderr || e.message || "").trim() : "",
        })
      },
    )
  })
}

const git = (...args) => run("git", args)

/**
 * Este clone pode receber atualização automática?
 *
 * As travas não são teóricas: a máquina de quem desenvolve o Arcadia usa o
 * MESMO clone que o app. Sem elas, o próprio app daria `git pull` por cima de
 * trabalho em andamento.
 */
async function estado() {
  if (!fs.existsSync(path.join(RAIZ, ".git"))) {
    return { podeAtualizar: false, motivo: "sem-git" }
  }

  const branch = await git("rev-parse", "--abbrev-ref", "HEAD")
  if (!branch.ok) return { podeAtualizar: false, motivo: "sem-git" }
  if (branch.saida !== BRANCH) {
    return { podeAtualizar: false, motivo: "branch", detalhe: branch.saida }
  }

  // `-uno`: só alteração em arquivo VERSIONADO bloqueia. Arquivo solto que o
  // usuário deixou na pasta não atrapalha um fast-forward — e contá-lo travava
  // a atualização de quem nunca mexeu em código nenhum.
  const sujo = await git("status", "--porcelain", "-uno")
  if (sujo.ok && sujo.saida) {
    const n = sujo.saida.split("\n").length
    return { podeAtualizar: false, motivo: "sujo", detalhe: String(n) }
  }

  // Commit local ainda não enviado: puxar por cima daria divergência, e o
  // trabalho de quem desenvolve viraria um merge no meio de uma partida.
  const naoEnviado = await git("rev-list", "--count", "@{u}..HEAD")
  if (naoEnviado.ok && Number(naoEnviado.saida) > 0) {
    return { podeAtualizar: false, motivo: "nao-enviado", detalhe: naoEnviado.saida }
  }

  return { podeAtualizar: true }
}

async function shaLocal() {
  const r = await git("rev-parse", "HEAD")
  return r.ok ? r.saida : ""
}

/**
 * Há commits novos no GitHub? Uma chamada resolve tudo.
 *
 * Atenção ao `ahead_by`, que parece o campo errado e não é: no `compare` a
 * base somos nós e a cabeça é o master, então quem está "à frente" é o
 * repositório — `behind_by` fica sempre em zero. Conferi na resposta real.
 *
 * O `files` diz se as dependências mudaram antes mesmo de puxar, e é o que
 * deixa o `npm install` rodar só quando é mesmo necessário.
 */
async function verificar() {
  const local = await shaLocal()
  if (!local) return { ok: false, error: "não é um clone git" }
  try {
    const r = await fetchRede(`${API}/compare/${local}...${BRANCH}`, {
      headers: { "User-Agent": "arcadia" },
      signal: AbortSignal.timeout(15000),
    })
    if (!r.ok) throw new Error(`GitHub HTTP ${r.status}`)
    const d = await r.json()
    const commits = (d.commits || [])
      .map((c) => ({
        sha: String(c.sha || "").slice(0, 7),
        titulo: String(c.commit?.message || "").split("\n")[0],
      }))
      .reverse() // o mais novo primeiro, que é a ordem que se lê
    return {
      ok: true,
      local: local.slice(0, 7),
      atrasado: Number(d.ahead_by) || 0,
      commits,
      depsMudaram: (d.files || []).some((f) => f.filename === "app/package-lock.json"),
    }
  } catch (e) {
    return { ok: false, error: String(e.message || e) }
  }
}

/**
 * Puxa, instala dependências se preciso e reconstrói. Quem reinicia é o main.
 *
 * Cada passo avisa pelo `onProgresso` para a tela não ficar muda: o pull de
 * uma atualização grande e o `npm install` levam dezenas de segundos.
 */
async function aplicar(onProgresso, depsMudaram = false) {
  const passo = (etapa) => onProgresso?.({ etapa })

  const st = await estado()
  if (!st.podeAtualizar) return { ok: false, error: `bloqueado: ${st.motivo}`, ...st }

  passo("pull")
  // --ff-only de propósito: se não avançar limpo, é para PARAR. Um merge ou
  // um reset automático aqui poderia descartar coisa do usuário.
  const pull = await git("pull", "--ff-only")
  if (!pull.ok) return { ok: false, error: pull.erro || "git pull falhou" }

  if (depsMudaram) {
    passo("deps")
    const npm = await run("npm", ["install", "--no-audit", "--no-fund"], { cwd: APP })
    if (!npm.ok) return { ok: false, error: npm.erro || "npm install falhou" }
  }

  passo("build")
  const build = await run("npm", ["run", "build"], { cwd: APP })
  // Build quebrado não pode virar reinício: o app continua de pé com o dist/
  // anterior e quem decide o que fazer é o usuário, vendo o erro.
  if (!build.ok) return { ok: false, error: build.erro || "build falhou" }

  passo("pronto")
  return { ok: true, sha: (await shaLocal()).slice(0, 7) }
}

module.exports = { estado, verificar, aplicar, shaLocal, RAIZ }
