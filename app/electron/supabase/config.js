// Configuracao do backend do Arcadia, servidor Node proprio.
// O URL aponta para o servidor local (ou do notebook via ARCADIA_SUPABASE_URL).
// A anonKey virou dummy. A seguranca agora vem do JWT + filtros no servidor.
// Override opcional por env vars (SUPABASE_URL/SUPABASE_ANON_KEY mantidos por
// compatibilidade. ARCADIA_SUPABASE_URL tambem e aceito).
module.exports = {
  url:
    process.env.ARCADIA_SUPABASE_URL ||
    process.env.SUPABASE_URL ||
    'http://127.0.0.1:3000',
  anonKey: process.env.SUPABASE_ANON_KEY || 'arcadia-dummy-key',
}
