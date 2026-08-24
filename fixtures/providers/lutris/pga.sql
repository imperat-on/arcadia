-- Minimal export of Lutris' pga.db schema used by the provider fixture test.
CREATE TABLE games (
    id INTEGER PRIMARY KEY,
    name TEXT,
    slug TEXT,
    runner TEXT,
    installed INTEGER,
    service TEXT,
    service_id TEXT
);
INSERT INTO games (id, name, slug, runner, installed, service, service_id) VALUES
    (10, 'Steam duplicate', 'steam-duplicate', 'steam', 1, 'steam', '440'),
    (11, 'Wine Game', 'wine-game', 'wine', 1, 'lutris', NULL),
    (12, NULL, 'fallback-title', 'linux', 1, 'lutris', ''),
    (13, 'Another duplicate', 'other', 'steam', 1, ' Steam ', '440');
