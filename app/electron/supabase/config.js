// Configuração do Supabase — Arcadia
// A publishable key é pública POR DESIGN (a segurança vem das políticas RLS).
// Override opcional por env vars (SUPABASE_URL / SUPABASE_ANON_KEY).
// Ver plano: docs/plans/2026-08-05-base-de-usuarios-plano.md (Fase 0.4)
module.exports = {
  url: process.env.SUPABASE_URL || 'https://ztvrvjezklorogrevmhg.supabase.co',
  anonKey:
    process.env.SUPABASE_ANON_KEY ||
    'sb_publishable_7ey2g6CBmmr2KCECuTje9A_b-LN8wlB',
}
