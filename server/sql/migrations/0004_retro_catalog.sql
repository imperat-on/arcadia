-- Catálogo Retro materializado. Cada rebuild é escrito com uma versão nova e
-- só se torna visível quando retro_catalog_state.active_version é atualizado.
CREATE TABLE IF NOT EXISTS retro_catalog_versions (
  version       TEXT PRIMARY KEY,
  status        TEXT NOT NULL CHECK (status IN ('building','active','superseded','failed')),
  generated_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
  source_meta   JSONB NOT NULL DEFAULT '{}'::jsonb,
  stats         JSONB NOT NULL DEFAULT '{}'::jsonb
);

CREATE TABLE IF NOT EXISTS retro_catalog_state (
  singleton      BOOLEAN PRIMARY KEY DEFAULT TRUE CHECK (singleton),
  active_version TEXT REFERENCES retro_catalog_versions(version),
  updated_at     TIMESTAMPTZ NOT NULL DEFAULT now()
);
INSERT INTO retro_catalog_state (singleton) VALUES (TRUE) ON CONFLICT DO NOTHING;

CREATE TABLE IF NOT EXISTS retro_games (
  version       TEXT NOT NULL REFERENCES retro_catalog_versions(version) ON DELETE CASCADE,
  game_id       TEXT NOT NULL,
  system_id     TEXT NOT NULL,
  title         TEXT NOT NULL,
  title_locale  TEXT NOT NULL DEFAULT 'en',
  sort_title    TEXT NOT NULL,
  search_text   TEXT NOT NULL,
  aliases       JSONB NOT NULL DEFAULT '[]'::jsonb,
  artwork       JSONB NOT NULL DEFAULT '{}'::jsonb,
  offer_count   INTEGER NOT NULL DEFAULT 0 CHECK (offer_count >= 0),
  match_quality TEXT NOT NULL DEFAULT 'strong',
  PRIMARY KEY (version, game_id)
);
CREATE INDEX IF NOT EXISTS idx_retro_games_page
  ON retro_games (version, system_id, sort_title, game_id);
CREATE INDEX IF NOT EXISTS idx_retro_games_search
  ON retro_games USING GIN (to_tsvector('simple', search_text));

CREATE TABLE IF NOT EXISTS retro_offers (
  version          TEXT NOT NULL,
  offer_id         TEXT NOT NULL,
  game_id          TEXT NOT NULL,
  source_id        TEXT NOT NULL,
  source_title     TEXT NOT NULL,
  original_title   TEXT NOT NULL,
  normalized_title TEXT NOT NULL,
  system_id        TEXT NOT NULL,
  metadata         JSONB NOT NULL DEFAULT '{}'::jsonb,
  uris             JSONB NOT NULL DEFAULT '[]'::jsonb,
  PRIMARY KEY (version, offer_id),
  FOREIGN KEY (version, game_id) REFERENCES retro_games(version, game_id) ON DELETE CASCADE
);
CREATE INDEX IF NOT EXISTS idx_retro_offers_game ON retro_offers (version, game_id);
