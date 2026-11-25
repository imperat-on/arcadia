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
//
// O endereço público é o Worker do Cloudflare (ver deploy/cloudflare-worker no
// repo do servidor): responde em IPv4 e IPv6 e não depende do DNS de quem usa o
// app. O Funnel do Tailscale fica como reserva — se o Worker cair, o cliente
// tenta o outro endereço antes de desistir (ver electron/httpfetch.js).
const DEFAULT_API_URL = "https://arcadiaserver.zesmehentperu.workers.dev"
const FALLBACK_API_URL = "https://zes.tail6e748d.ts.net"

const override =
  normalizeUrl(process.env.ARCADIA_API_URL) ||
  normalizeUrl(process.env.ARCADIA_SUPABASE_URL) ||
  normalizeUrl(process.env.SUPABASE_URL)

const url = override || DEFAULT_API_URL

// Com override (servidor próprio) não faz sentido cair no backend oficial:
// a lista é só o endereço escolhido.
const urls = override ? [override] : [...new Set([DEFAULT_API_URL, FALLBACK_API_URL])]

module.exports = {
  url,
  urls,
  DEFAULT_API_URL,
  FALLBACK_API_URL,
  anonKey: process.env.SUPABASE_ANON_KEY || "arcadia-dummy-key",
  normalizeUrl,
}
