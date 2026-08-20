-- Arcadia migration 0002: índices para operações por conta e deltas.
-- O baseline já criou as tabelas; esta migration é segura em upgrades e novos
-- bancos. Não remova índices antigos sem medir as consultas de produção.

CREATE INDEX IF NOT EXISTS idx_friendships_status_pair
  ON friendships (status, user_a, user_b);

CREATE INDEX IF NOT EXISTS idx_user_library_user_updated
  ON user_library (user_id, updated_at);

CREATE INDEX IF NOT EXISTS idx_user_playtime_user_updated
  ON user_playtime (user_id, updated_at);

CREATE INDEX IF NOT EXISTS idx_user_sources_active
  ON user_sources (user_id, removed_at);
