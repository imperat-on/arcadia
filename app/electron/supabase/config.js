// Configuração do backend do Arcadia, servidor Node próprio.
// `ARCADIA_API_URL` é o nome canônico; os nomes Supabase continuam aceitos
// somente como compatibilidade com instalações anteriores.
function normalizeUrl(value) {
  const url = String(value || "").trim().replace(/\/+$/, "")
  return url || null
}

// Instância oficial usada pelas instalações do Arcadia. O override por
// ARCADIA_API_URL continua disponível para desenvolvimento e servidores
// próprios; sem ele, o launcher não depende de um backend local.
const DEFAULT_API_URL = "https://zes.tail6e748d.ts.net"

const url =
  normalizeUrl(process.env.ARCADIA_API_URL) ||
  normalizeUrl(process.env.ARCADIA_SUPABASE_URL) ||
  normalizeUrl(process.env.SUPABASE_URL) ||
  DEFAULT_API_URL

module.exports = {
  url,
  DEFAULT_API_URL,
  anonKey: process.env.SUPABASE_ANON_KEY || "arcadia-dummy-key",
  normalizeUrl,
}
