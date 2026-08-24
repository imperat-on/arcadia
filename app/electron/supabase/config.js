// Configuração do backend do Arcadia, servidor Node próprio.
// `ARCADIA_API_URL` é o nome canônico; os nomes Supabase continuam aceitos
// somente como compatibilidade com instalações anteriores.
function normalizeUrl(value) {
  const url = String(value || "").trim().replace(/\/+$/, "")
  return url || null
}

const url =
  normalizeUrl(process.env.ARCADIA_API_URL) ||
  normalizeUrl(process.env.ARCADIA_SUPABASE_URL) ||
  normalizeUrl(process.env.SUPABASE_URL) ||
  "http://127.0.0.1:3000"

module.exports = {
  url,
  anonKey: process.env.SUPABASE_ANON_KEY || "arcadia-dummy-key",
  normalizeUrl,
}
