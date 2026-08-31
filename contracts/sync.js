"use strict"

// Regras de merge do sync. Este arquivo é deliberadamente puro: não lê disco,
// não consulta relógio/rede e não importa Electron. O main process usa as
// funções para aplicar a mesma decisão em enqueue/pull; o servidor continua
// sendo a autoridade final para os RPCs.

const MAX_SYNC_MINUTES = 999999
const MAX_SYNC_TIMESTAMP = 8640000000 // ano 2243 em epoch-segundos

function finiteNumber(value) {
  if (typeof value === "boolean" || value == null) return null
  const n = Number(value)
  return Number.isFinite(n) ? n : null
}

/** Epoch em segundos; aceita segundos, milissegundos e datas ISO. */
function normalizeSyncTimestamp(value) {
  if (value == null || value === "") return null
  let number = finiteNumber(value)
  if (number == null && typeof value === "string") {
    const parsed = Date.parse(value)
    number = Number.isFinite(parsed) ? parsed : null
  }
  if (number == null || number <= 0) return null
  // Datas em milissegundos têm pelo menos 12 dígitos hoje. O limite abaixo
  // evita converter um timestamp obviamente corrompido em um valor válido.
  if (number > 1e12) number = Math.floor(number / 1000)
  number = Math.floor(number)
  return Number.isSafeInteger(number) && number > 0 && number <= MAX_SYNC_TIMESTAMP
    ? number
    : null
}

function text(value, max = 512) {
  if (typeof value !== "string" && typeof value !== "number") return null
  const result = String(value).trim()
  return result && result.length <= max ? result : null
}

function stableValue(value) {
  if (Array.isArray(value)) return value.map(stableValue)
  if (value && typeof value === "object") {
    return Object.keys(value)
      .sort()
      .reduce((out, key) => {
        out[key] = stableValue(value[key])
        return out
      }, {})
  }
  return value
}

function stableString(value) {
  try {
    return JSON.stringify(stableValue(value))
  } catch {
    return ""
  }
}

function numberValue(value, { min = 0, max = Number.MAX_SAFE_INTEGER } = {}) {
  const n = finiteNumber(value)
  if (n == null || !Number.isSafeInteger(n) || n < min || n > max) return null
  return n
}

function achievementValue(value) {
  if (!value || typeof value !== "object" || Array.isArray(value)) return null
  const appid = text(value.appid)
  const apiname = text(value.apiname)
  if (!appid || !apiname) return null
  // unlocked_at=0 é válido para conquistas bloqueadas (synced mas não desbloqueadas).
  // normalizeSyncTimestamp retorna null para 0, então tratamos 0 separadamente.
  const rawTs = value.unlocked_at ?? value.unlock
  const unlockedAt = rawTs === 0 ? 0 : normalizeSyncTimestamp(rawTs)
  // achieved pode vir como boolean explícito ou ser derivado de unlocked_at
  const achieved = value.achieved === true || (unlockedAt != null && unlockedAt > 0) || value.unlocked === true
  return {
    ...value,
    appid,
    apiname,
    achieved,
    unlocked_at: unlockedAt,
  }
}

function metadataWinner(left, right) {
  const leftJson = stableString(left)
  const rightJson = stableString(right)
  // Comparação lexical não depende da ordem de chegada dos deltas.
  return leftJson <= rightJson ? left : right
}

/**
 * Merge de conquistas: desbloqueio é monotônico e o menor timestamp vence.
 * Empates/metadata são resolvidos por JSON canônico, portanto duas máquinas
 * que recebem os deltas em ordens diferentes convergem para o mesmo valor.
 */
function resolveAchievementConflict(local, remote) {
  const left = achievementValue(local)
  const right = achievementValue(remote)
  if (!left) return right
  if (!right) return left
  if (left.appid !== right.appid || left.apiname !== right.apiname) return null

  const leftTs = left.unlocked_at
  const rightTs = right.unlocked_at
  let winner
  if (leftTs == null && rightTs != null) winner = right
  else if (rightTs == null && leftTs != null) winner = left
  else if (leftTs != null && rightTs != null && leftTs !== rightTs) {
    // Para conquistas desbloqueadas (ts > 0), menor vence.
    // Para bloqueadas (ts = 0), qualquer valor não-zero vence.
    if (leftTs > 0 && rightTs > 0) winner = leftTs < rightTs ? left : right
    else if (leftTs > 0) winner = left
    else if (rightTs > 0) winner = right
    else winner = metadataWinner(left, right)
  } else winner = metadataWinner(left, right)

  // Merge timestamps:ignorar 0 (locked) ao calcular menor timestamp
  const nonZero = [leftTs, rightTs].filter((t) => t != null && t > 0)
  const timestamp = nonZero.length ? Math.min(...nonZero) : (leftTs ?? rightTs ?? null)
  // achieved é monotônico: se qualquer lado desbloqueou, fica desbloqueado.
  // Timestamp usa earliest-wins para desbloqueios (menor > 0 vence).
  return {
    ...winner,
    appid: left.appid,
    apiname: left.apiname,
    achieved: left.achieved || right.achieved || (timestamp != null && timestamp > 0),
    unlocked_at: timestamp,
  }
}

function operationRevision(value) {
  if (!value || typeof value !== "object") return null
  // revision/lamport is preferred to wall-clock values when supplied. The
  // aliases let old queue records and future contracts coexist.
  for (const key of ["revision", "lamport", "version"]) {
    const n = numberValue(value[key], { min: 0 })
    if (n != null) return n
  }
  for (const key of ["updated_at", "updatedAt", "operation_at", "operationAt", "created_at", "createdAt"]) {
    const n = normalizeSyncTimestamp(value[key])
    if (n != null) return n
  }
  return null
}

function libraryValue(value) {
  if (!value || typeof value !== "object" || Array.isArray(value)) return null
  const appid = text(value.appid ?? value.id)
  if (!appid) return null
  const removed = value.removed === true
  const title = text(value.title, 1024)
  const platform = value.platform === "emulator" ? "emulator" : value.platform === "linux" ? "linux" : "windows"
  return {
    ...value,
    appid,
    title: title || appid,
    platform,
    removed,
  }
}

function titleQuality(value) {
  const title = String(value?.title || "").trim()
  const appid = String(value?.appid || "").trim()
  if (!title || title === appid || /^steam \d+$/i.test(title)) return 0
  return 1
}

/**
 * Merge de biblioteca por appid.
 *
 * A operação com maior revision/timestamp vence. Quando versões são iguais
 * (ou o backend antigo não fornece versão), remoção vence para não ressuscitar
 * um jogo apagado offline; dois upserts empatados usam representação canônica.
 * A política é intencionalmente conservadora e determinística.
 */
function resolveLibraryConflict(local, remote) {
  const left = libraryValue(local)
  const right = libraryValue(remote)
  if (!left) return right
  if (!right) return left
  if (left.appid !== right.appid) return null

  const leftRevision = operationRevision(left)
  const rightRevision = operationRevision(right)
  let winner
  if (leftRevision != null && rightRevision != null && leftRevision !== rightRevision) {
    winner = leftRevision > rightRevision ? left : right
  } else if (leftRevision != null && rightRevision == null) {
    winner = left
  } else if (rightRevision != null && leftRevision == null) {
    winner = right
  } else if (left.removed !== right.removed) {
    // Uma remoção explícita vence o empate para impedir que um pull atrasado
    // ressuscite o jogo apagado offline.
    winner = left.removed ? left : right
  } else if (titleQuality(left) !== titleQuality(right)) {
    // IDs/nomes sintéticos não devem substituir um título real vindo da
    // loja, mesmo quando o backend antigo não traz versão.
    winner = titleQuality(left) > titleQuality(right) ? left : right
  } else {
    winner = metadataWinner(left, right)
  }
  return { ...winner, appid: left.appid, removed: Boolean(winner.removed) }
}

function playtimeMinutes(value) {
  const raw = value && typeof value === "object" ? value.minutes : value
  return numberValue(raw, { min: 0, max: MAX_SYNC_MINUTES }) ?? 0
}

/**
 * Horas são totais monotônicos no cliente: ao aplicar um pull, o maior total
 * preserva minutos jogados localmente e remotamente. O RPC recebe deltas e
 * acumula cada máquina; este merge só decide o display local.
 */
function resolvePlaytimeConflict(local, remote) {
  return Math.max(playtimeMinutes(local), playtimeMinutes(remote))
}

function resolveSyncConflict(kind, local, remote) {
  switch (String(kind || "").toLowerCase()) {
    case "achievement":
    case "achievements":
      return resolveAchievementConflict(local, remote)
    case "library":
    case "game":
      return resolveLibraryConflict(local, remote)
    case "playtime":
    case "minutes":
      return resolvePlaytimeConflict(local, remote)
    default:
      return null
  }
}

module.exports = {
  MAX_SYNC_MINUTES,
  normalizeSyncTimestamp,
  resolveAchievementConflict,
  resolveLibraryConflict,
  resolvePlaytimeConflict,
  resolveSyncConflict,
}
