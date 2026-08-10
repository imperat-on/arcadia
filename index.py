#!/usr/bin/env python3
"""Indexador de bibliotecas para o front-end PS5.

Varre Steam, Heroic (Epic/GOG/Amazon) e Lutris e gera um `library.json`
unificado que o app Godot consome. Cada jogo vira:

    {
      "id":        "steam:440",
      "title":     "Team Fortress 2",
      "launcher":  "steam",
      "launch_cmd": ["steam", "steam://rungameid/440"],
      "cover":     "/caminho/library_600x900.jpg" | "",
      "hero":      "/caminho/library_hero.jpg"     | "",
      "logo":      "/caminho/logo.png"             | ""
    }

Sem dependências obrigatórias: usa `python-vdf` se existir, senão cai num
parser mínimo próprio. Bibliotecas vazias (ex.: Heroic sem login) são ignoradas
sem erro.
"""

from __future__ import annotations

import json
import os
import re
import sqlite3
import sys
import time
import urllib.error
import urllib.request
from concurrent.futures import ThreadPoolExecutor
from pathlib import Path

# Concorrência do enriquecimento (Steam Store / SteamGridDB / Web API). Pool
# pequeno: paraleliza o gargalo de rede da primeira execução sem estourar o
# rate limit dos endpoints. Cada worker só faz rede e devolve resultado; o
# merge nos caches roda serial na thread principal (sem lock).
ENRICH_WORKERS = 6

HOME = Path.home()
OUT_DIR = HOME / ".local/share/arcadia"
OUT_FILE = OUT_DIR / "library.json"

# Steam
def _steam_root() -> Path:
    """Raiz da Steam. O 1o candidato com libraryfolders.vdf, senao o 1o com
    steamapps/ (marca de instalacao real, nao um dir parcial)."""
    cands = [HOME / d for d in (
        ".steam/steam",
        ".steam/root",
        ".local/share/Steam",
        ".var/app/com.valvesoftware.Steam/.local/share/Steam",
        ".steam/debian-installation",
    )]
    if env := os.environ.get("STEAM_BASE_FOLDER"):
        cands.insert(0, Path(env).expanduser())
    cands = [c.resolve() for c in cands]
    for c in cands:
        if (c / "steamapps/libraryfolders.vdf").is_file():
            return c
    for c in cands:
        if (c / "steamapps").is_dir():
            print(f"[aviso] Steam: sem libraryfolders.vdf; usando {c} (pode nao listar todos os drives).", file=sys.stderr)
            return c
    print("[aviso] Steam: raiz nao encontrada (instalacao ausente?).", file=sys.stderr)
    return HOME / ".local/share/Steam"

STEAM_ROOT = _steam_root()
STEAM_LIBCACHE = STEAM_ROOT / "appcache/librarycache"
STEAM_USERDATA = STEAM_ROOT / "userdata"
STEAM_CDN = "https://cdn.cloudflare.steamstatic.com/steam/apps"

# Config opcional: ~/.local/share/arcadia/config.json
#   { "steam_api_key": "SUA_CHAVE", "steam_id64": "opcional" }
CONFIG_FILE = OUT_DIR / "config.json"

_STEAM_LANG = None

def _get_steam_lang() -> str:
    global _STEAM_LANG
    if _STEAM_LANG is not None:
        return _STEAM_LANG
    cfg = load_config()
    lang = (cfg.get("language") or "en-US").strip()
    MAP = {
        "pt-BR": "portuguese",
        "en-US": "english",
        "es-ES": "spanish",
    }
    _STEAM_LANG = MAP.get(lang, "english")
    return _STEAM_LANG

# SLSsteam: jogos injetados ficam no bloco AdditionalApps do config.yaml.
SLS_CONFIG = HOME / ".config/SLSsteam/config.yaml"

# appids que são ferramentas/runtimes, não jogos (não mostrar).
STEAM_TOOL_IDS = {
    "228980",   # Steamworks Common Redistributables
    "1493710",  # Proton Experimental
    "1070560",  # Steam Linux Runtime 1.0 (scout)
    "1391110",  # Steam Linux Runtime 2.0 (soldier)
    "1628350",  # Steam Linux Runtime 3.0 (sniper)
    "1826330",  # Proton EasyAntiCheat Runtime
    "2180100",  # Proton Hotfix
}
STEAM_TOOL_WORDS = ("runtime", "redistributable", "proton", "steamworks")


# --------------------------------------------------------------------------- #
# VDF: usa a lib se houver, senão parser mínimo (chave/valor aninhado).
# --------------------------------------------------------------------------- #
try:
    import vdf  # type: ignore

    def parse_vdf(text: str) -> dict:
        return vdf.loads(text)
except Exception:  # pragma: no cover - fallback simples
    def parse_vdf(text: str) -> dict:
        root: dict = {}
        stack = [root]
        key_pat = re.compile(r'"((?:[^"\\]|\\.)*)"')
        for line in text.splitlines():
            line = line.strip()
            if not line or line.startswith("//"):
                continue
            if line == "{":
                continue
            if line == "}":
                if len(stack) > 1:
                    stack.pop()
                continue
            keys = key_pat.findall(line)
            if len(keys) >= 2:
                stack[-1][keys[0]] = keys[1]
            elif len(keys) == 1:
                child: dict = {}
                stack[-1][keys[0]] = child
                stack.append(child)
        return root


def _read(path: Path) -> str | None:
    try:
        return path.read_text(encoding="utf-8", errors="replace")
    except OSError:
        return None


def _atomic_write(path: Path, text: str) -> bool:
    """Escreve arquivo de forma atômica (tmp + rename). True em sucesso."""
    try:
        path.parent.mkdir(parents=True, exist_ok=True)
        tmp = path.with_suffix(path.suffix + ".tmp")
        tmp.write_text(text, encoding="utf-8")
        os.replace(tmp, path)
        return True
    except OSError:
        return False


# --------------------------------------------------------------------------- #
# Steam
# --------------------------------------------------------------------------- #
def steam_library_dirs() -> list[Path]:
    """Todas as pastas steamapps (drive principal + extras do libraryfolders)."""
    dirs: list[Path] = []
    main = STEAM_ROOT / "steamapps"
    if main.is_dir():
        dirs.append(main)
    lf = main / "libraryfolders.vdf"
    text = _read(lf)
    if text:
        data = parse_vdf(text)
        folders = data.get("libraryfolders", data)
        if isinstance(folders, dict):
            for entry in folders.values():
                if isinstance(entry, dict) and "path" in entry:
                    p = Path(entry["path"]) / "steamapps"
                    if p.is_dir() and p not in dirs:
                        dirs.append(p)
    return dirs


def _find_asset(folder: Path, names: tuple[str, ...]) -> str:
    """Procura o 1º arquivo com um dos nomes dados, direto ou em subpasta-hash
    (o Steam mais novo guarda a arte em `<appid>/<hash>/library_600x900.jpg`)."""
    if not folder.is_dir():
        return ""
    for name in names:
        direct = folder / name
        if direct.exists():
            return str(direct)
        for hit in folder.glob(f"*/{name}"):
            return str(hit)
    return ""


def steam_art(appid: str) -> dict:
    folder = STEAM_LIBCACHE / appid
    return {
        "cover": _find_asset(folder, ("library_600x900.jpg",)),
        "hero": _find_asset(folder, ("library_hero.jpg",)),
        "logo": _find_asset(folder, ("logo.png",)),
    }


def index_steam() -> list[dict]:
    games: list[dict] = []
    seen: set[str] = set()
    for steamapps in steam_library_dirs():
        for acf in steamapps.glob("appmanifest_*.acf"):
            text = _read(acf)
            if not text:
                continue
            data = parse_vdf(text).get("AppState", {})
            appid = str(data.get("appid", "")).strip()
            name = str(data.get("name", "")).strip()
            if not appid or appid in seen:
                continue
            if appid in STEAM_TOOL_IDS:
                continue
            if any(w in name.lower() for w in STEAM_TOOL_WORDS):
                continue
            seen.add(appid)
            art = steam_art(appid)
            games.append({
                "id": f"steam:{appid}",
                "title": name or f"App {appid}",
                "launcher": "steam",
                "launch_cmd": ["steam", f"steam://rungameid/{appid}"],
                "installed": True,
                **art,
            })
    return games


META_CACHE = OUT_DIR / "meta_cache.json"


def _load_meta_cache() -> dict:
    try:
        return json.loads(META_CACHE.read_text(encoding="utf-8"))
    except Exception:
        return {}


def _save_meta_cache(cache: dict) -> None:
    _atomic_write(META_CACHE, json.dumps(cache, ensure_ascii=False))


def _url_ok(url: str) -> bool | None:
    """HEAD rápido: True se existe (200), False se confirmadamente ausente
    (ex.: 404), None se não foi possível checar (erro de rede/timeout)."""
    import urllib.error
    import urllib.request
    try:
        req = urllib.request.Request(
            url, method="HEAD", headers={"User-Agent": "Mozilla/5.0"})
        with urllib.request.urlopen(req, timeout=10) as r:
            return getattr(r, "status", r.getcode()) == 200
    except urllib.error.HTTPError as e:
        return False if e.code == 404 else None
    except Exception:
        return None


def sgdb_hero(appid: str, key: str) -> str | None:
    """Melhor hero (maior resolução) do SteamGridDB para um appid da Steam.

    Devolve "" quando confirmadamente não há hero melhor, None quando a
    consulta falhou (rede/timeout) e deve ser retentada depois."""
    import urllib.parse
    import urllib.request
    base = "https://www.steamgriddb.com/api/v2"
    hdr = {"Authorization": f"Bearer {key}", "User-Agent": "Mozilla/5.0"}
    try:
        req = urllib.request.Request(f"{base}/games/steam/{appid}", headers=hdr)
        with urllib.request.urlopen(req, timeout=15) as r:
            gid = json.loads(r.read().decode("utf-8")).get("data", {}).get("id")
        if not gid:
            return ""
        q = urllib.parse.urlencode({"dimensions": "3840x1240,1920x620"})
        req = urllib.request.Request(
            f"{base}/heroes/game/{gid}?{q}", headers=hdr)
        with urllib.request.urlopen(req, timeout=15) as r:
            arr = json.loads(r.read().decode("utf-8")).get("data", []) or []
        arr = [a for a in arr if a.get("url")]
        arr.sort(key=lambda a: a.get("width", 0), reverse=True)
        return arr[0]["url"] if arr else ""
    except Exception:
        return None


def best_steam_hero(appid: str, current: str, sgdb_key: str) -> tuple[str, bool]:
    """Escolhe o hero de maior resolução: 2x da Steam > SteamGridDB > atual.

    Devolve (url, ok). `url` é "" quando nada melhor foi encontrado (mantém
    o `current`). `ok=False` indica falha transitória (rede/timeout): quem
    chama não deve cachear o resultado, para tentar de novo depois."""
    two_x = f"{STEAM_CDN}/{appid}/library_hero_2x.jpg"  # 3840x1240 quando existe
    status = _url_ok(two_x)
    if status is None:
        return "", False
    if status:
        return two_x, True
    if sgdb_key:
        hero = sgdb_hero(appid, sgdb_key)
        if hero is None:
            return "", False
        if hero:
            return hero, True
    return "", True


def fetch_appdetails(appid: str, lang: str = "") -> dict | None:
    """Descrição/gênero/ano/nota via Steam Store API (público, sem chave)."""
    import urllib.parse
    import urllib.request
    api_lang = lang or _get_steam_lang()
    url = "https://store.steampowered.com/api/appdetails?" + \
        urllib.parse.urlencode({"appids": appid, "l": api_lang})
    try:
        req = urllib.request.Request(url, headers={"User-Agent": "Mozilla/5.0"})
        with urllib.request.urlopen(req, timeout=15) as r:
            data = json.loads(r.read().decode("utf-8"))
        node = data.get(str(appid), {})
        if not node.get("success"):
            # App delistado/sem página: cacheia marcador válido para não refazer.
            return {"_v": 3, "_lang": api_lang, "_missing": True}
        info = node.get("data", {})
        genres = [g.get("description") for g in info.get("genres", [])
                  if g.get("description")]
        year = ""
        m = re.search(r"(\d{4})", info.get("release_date", {}).get("date", ""))
        if m:
            year = m.group(1)
        rating = 0.0
        mc = info.get("metacritic", {}).get("score")
        if isinstance(mc, (int, float)) and mc > 0:
            rating = round(mc / 20.0, 1)
        # Categorias multijogador (ids conhecidos da Store: 1 single, 9 co-op,
        # 27/36/38 multi/pvp…) — resume em texto curto para o overview.
        cats = {c.get("id") for c in info.get("categories", [])}
        PLAYER_LABELS = {
            "portuguese": {1: "1 jogador", 9: "co-op", 38: "co-op", 27: "multiplayer", 36: "multiplayer", 49: "multiplayer"},
            "english":    {1: "single player", 9: "co-op", 38: "co-op", 27: "multiplayer", 36: "multiplayer", 49: "multiplayer"},
            "spanish":    {1: "1 jugador", 9: "cooperativo", 38: "cooperativo", 27: "multijugador", 36: "multijugador", 49: "multijugador"},
        }
        labels = PLAYER_LABELS.get(api_lang, PLAYER_LABELS["english"])
        modos = []
        if 1 in cats:
            modos.append(labels[1])
        if cats & {9, 38}:
            modos.append(labels[9])
        if cats & {27, 36, 49}:
            modos.append(labels[27])
        return {
            "_v": 3,
            "_lang": api_lang,
            "name": info.get("name", "") or "",
            "description": info.get("short_description", "") or "",
            "genre": ", ".join(genres[:2]),
            "year": year,
            "rating": rating,
            "developer": ", ".join(info.get("developers", [])[:2]),
            "publisher": ", ".join(info.get("publishers", [])[:2]),
            "metacritic": mc if isinstance(mc, (int, float)) and mc > 0 else 0,
            "players": " · ".join(modos),
        }
    except Exception:
        return None


def enrich_steam(games: list[dict], sgdb_key: str = "", lang: str = "") -> None:
    """Adiciona metadados aos jogos Steam, com cache em meta_cache.json."""
    cache = _load_meta_cache()
    changed = False
    api_lang = lang or _get_steam_lang()

    def resolver(g: dict):
        """Só rede: devolve (appid, meta_final, mudou). Sem tocar em `cache`
        nem em `g` — a thread principal faz o merge depois."""
        gid = g.get("id", "")
        if ":" not in gid:
            return "", None, False
        appid = gid.split(":", 1)[1]
        meta = cache.get(appid)
        if not isinstance(meta, dict):
            meta = None
        # Cache da versão antiga (sem os campos novos) ou idioma diferente: refaz.
        hero_hd = meta.get("_hero_hd") if meta is not None else None
        if meta is not None and (meta.get("_v") != 3 or meta.get("_lang") != api_lang):
            meta = None
        mudou = False
        if meta is None:
            meta = fetch_appdetails(appid, api_lang)
            if meta is None:  # erro de rede: não cacheia (tenta de novo depois)
                return appid, None, False
            # O hero em alta não depende do idioma: sem carregá-lo adiante,
            # trocar de idioma refazia a busca no SteamGridDB da biblioteca
            # inteira, de graça.
            if hero_hd:
                meta["_hero_hd"] = hero_hd
            mudou = True
        else:
            meta = dict(meta)  # cópia: não muta a entrada compartilhada do cache
        # Hero em alta resolução (2x da Steam > SteamGridDB), resolvido 1x e cacheado.
        if "_hero_hd" not in meta:
            hd, hero_ok = best_steam_hero(appid, g.get("hero", ""), sgdb_key)
            if hero_ok:  # só cacheia resultado confirmado; falha transitória tenta de novo depois
                meta["_hero_hd"] = hd
                mudou = True
        return appid, meta, mudou

    alvos = [g for g in games if g.get("launcher") == "steam"]
    porAppid = {}
    for g in alvos:
        gid = g.get("id", "")
        if ":" in gid:
            porAppid[gid.split(":", 1)[1]] = g
    with ThreadPoolExecutor(max_workers=ENRICH_WORKERS) as ex:
        for appid, meta, mudou in ex.map(resolver, alvos):
            if meta is None:
                continue
            if mudou:
                cache[appid] = meta
                changed = True
            g = porAppid[appid]
            if meta.get("_hero_hd"):
                g["hero"] = meta["_hero_hd"]
            # Preenche o título de jogos injetados (SLSsteam) que vieram sem nome.
            if meta.get("name") and (not g.get("title") or g["title"].startswith("App ")):
                g["title"] = meta["name"]
            for k, v in meta.items():
                if k == "name" or k.startswith("_"):
                    continue
                if v:
                    g[k] = v
    if changed:
        _save_meta_cache(cache)


def load_config() -> dict:
    try:
        return json.loads(CONFIG_FILE.read_text(encoding="utf-8"))
    except Exception:
        return {}


# --------------------------------------------------------------------------- #
# PlayStation Network (PSN)
# Fluxo (igual ao plugin do Playnite / à lib psn-api):
#   npsso (o usuário cola)  ->  code OAuth  ->  access_token  ->  API de títulos.
# São jogos de console: NÃO rodam no PC. Entram como coleção; "jogar" abre a
# página do jogo na PS Store (ou use o Chiaki p/ Remote Play do seu console).
# --------------------------------------------------------------------------- #
PSN_CLIENT_ID = "09515159-7237-4370-9b40-3806e67c0891"
PSN_CLIENT_AUTH = "MDk1MTUxNTktNzIzNy00MzcwLTliNDAtMzgwNmU2N2MwODkxOnVjUGprYTV0bnRCMktxc1A="
PSN_REDIRECT = "com.scee.psxandroid.scecompcall://redirect"
PSN_AUTHORIZE = "https://ca.account.sony.com/api/authz/v3/oauth/authorize"
PSN_TOKEN = "https://ca.account.sony.com/api/authz/v3/oauth/token"
PSN_TITLES = ("https://m.np.playstation.com/api/gamelist/v2/users/me/titles"
              "?categories=ps4_game,ps5_native_game&limit=200&offset={0}")


class _NoRedirect(urllib.request.HTTPRedirectHandler):
    """Impede o urllib de seguir o redirect (o destino é um esquema app://)."""
    def redirect_request(self, *args, **kwargs):
        return None


def psn_access_token(npsso: str) -> str:
    """Troca o npsso pelo access_token via code OAuth. "" em caso de falha."""
    import urllib.error
    import urllib.parse
    import urllib.request
    params = urllib.parse.urlencode({
        "access_type": "offline",
        "client_id": PSN_CLIENT_ID,
        "redirect_uri": PSN_REDIRECT,
        "response_type": "code",
        "scope": "psn:mobile.v2.core psn:clientapp",
    })
    # 1) authorize com o cookie npsso -> o header Location traz ?code=...
    opener = urllib.request.build_opener(_NoRedirect())
    req = urllib.request.Request(
        f"{PSN_AUTHORIZE}?{params}",
        headers={"Cookie": f"npsso={npsso}", "User-Agent": "Mozilla/5.0"})
    location = ""
    try:
        location = opener.open(req, timeout=20).headers.get("Location", "")
    except urllib.error.HTTPError as e:  # 302 vem como "erro" com o Location
        location = e.headers.get("Location", "") or ""
    except Exception:
        return ""
    code = urllib.parse.parse_qs(
        urllib.parse.urlparse(location).query).get("code", [""])[0]
    if not code:
        return ""
    # 2) code -> access_token
    body = urllib.parse.urlencode({
        "code": code,
        "redirect_uri": PSN_REDIRECT,
        "grant_type": "authorization_code",
        "token_format": "jwt",
    }).encode()
    req = urllib.request.Request(PSN_TOKEN, data=body, headers={
        "Authorization": f"Basic {PSN_CLIENT_AUTH}",
        "Content-Type": "application/x-www-form-urlencoded",
        "User-Agent": "Mozilla/5.0",
    })
    try:
        with urllib.request.urlopen(req, timeout=20) as r:
            return json.loads(r.read().decode("utf-8")).get("access_token", "") or ""
    except Exception:
        return ""


def psn_titles(token: str) -> list[dict]:
    """Todos os títulos jogados (PS4/PS5) com nome e capa, paginado."""
    import urllib.request
    out: list[dict] = []
    offset = 0
    while True:
        req = urllib.request.Request(
            PSN_TITLES.format(offset),
            headers={"Authorization": f"Bearer {token}",
                     "User-Agent": "Mozilla/5.0"})
        try:
            with urllib.request.urlopen(req, timeout=20) as r:
                data = json.loads(r.read().decode("utf-8"))
        except Exception as e:
            print(f"[aviso] PSN títulos: {e}", file=sys.stderr)
            break
        out.extend(data.get("titles", []) or [])
        nxt = data.get("nextOffset")
        try:
            nxt_int = int(nxt) if nxt is not None else 0
        except (TypeError, ValueError):
            break
        if nxt_int <= offset:
            break
        offset = nxt_int
    return out


def index_psn(npsso: str) -> list[dict]:
    """Biblioteca da PSN. Jogos de console: launch abre a PS Store."""
    import urllib.parse
    npsso = (npsso or "").strip()
    if not npsso:
        return []
    token = psn_access_token(npsso)
    if not token:
        print("[aviso] PSN: npsso inválido ou expirado (pegue um novo).",
              file=sys.stderr)
        return []
    titles = psn_titles(token)
    if not titles:
        print("[aviso] PSN: conectou, mas a conta retornou 0 jogos. Verifique se "
              "o npsso é da MESMA conta do console e se 'Jogos/Histórico' e "
              "'Troféus' estão como 'Qualquer pessoa' na privacidade da PSN.",
              file=sys.stderr)
    games: list[dict] = []
    for t in titles:
        title = t.get("name") or t.get("localizedName") or ""
        if not title:
            continue
        tid = t.get("titleId") or title
        img = t.get("imageUrl") or ""
        concept = (t.get("concept") or {}).get("id")
        if concept:
            store = f"https://store.playstation.com/concept/{concept}"
        else:
            store = "https://store.playstation.com/search/" + \
                urllib.parse.quote(title)
        games.append({
            "id": f"psn:{tid}",
            "title": title,
            "launcher": "psn",
            "launch_cmd": ["xdg-open", store],
            "cover": img,
            "hero": "",
            "logo": "",
            "installed": False,
        })
    return games


PLAYER_CACHE = OUT_DIR / "player_cache.json"
PLAYER_TTL = 24 * 3600  # playtime revalida 1x por dia


def enrich_player(games: list[dict], cfg: dict) -> None:
    """Tempo de jogo (playtime_minutes) do jogador, via Steam Web API.

    Usa GetOwnedGames (1 chamada, cache 24h em player_cache.json).
    """
    key = (cfg.get("steam_api_key") or "").strip()
    if not key:
        return
    sid = (cfg.get("steam_id64") or "").strip() or steam_id64()
    if not sid:
        return

    owned = {str(o.get("appid", "")): o for o in steam_owned(key, sid)}
    try:
        cache = json.loads(PLAYER_CACHE.read_text(encoding="utf-8"))
    except Exception:
        cache = {}
    now = time.time()
    changed = False

    for g in games:
        if g.get("launcher") != "steam":
            continue
        gid = g.get("id", "")
        if ":" not in gid:
            continue
        appid = gid.split(":", 1)[1]
        og = owned.get(appid)
        if not og:
            continue
        ent = cache.get(appid)
        if isinstance(ent, dict) and now - ent.get("at", 0) <= PLAYER_TTL:
            mins = ent.get("playtime_minutes", 0)
        else:
            mins = (og.get("playtime_forever") or 0) // 60
            cache[appid] = {"playtime_minutes": mins, "at": now}
            changed = True
        g["playtime_minutes"] = mins

    if changed:
        _atomic_write(PLAYER_CACHE, json.dumps(cache, ensure_ascii=False))


def steam_id64() -> str | None:
    """Deriva o SteamID64 a partir da conta ativa (ver `_active_account_id`)."""
    acct = _active_account_id()
    return str(76561197960265728 + int(acct)) if acct else None


def steam_owned(api_key: str, steamid64: str) -> list[dict]:
    """Lista TODA a biblioteca (instalados ou não) via Steam Web API."""
    import urllib.parse
    import urllib.request
    url = "https://api.steampowered.com/IPlayerService/GetOwnedGames/v1/?" + \
        urllib.parse.urlencode({
            "key": api_key, "steamid": steamid64,
            "include_appinfo": 1, "include_played_free_games": 1,
            "format": "json",
        })
    try:
        with urllib.request.urlopen(url, timeout=20) as r:
            data = json.loads(r.read().decode("utf-8"))
    except Exception as e:
        print(f"[aviso] Steam GetOwnedGames falhou: {e}", file=sys.stderr)
        return []
    return data.get("response", {}).get("games", []) or []


def steam_owned_games(installed_ids: set[str]) -> list[dict]:
    """Jogos possuídos mas NÃO instalados (arte via CDN). Requer chave da API."""
    cfg = load_config()
    key = (cfg.get("steam_api_key") or "").strip()
    if not key:
        return []
    sid = (cfg.get("steam_id64") or "").strip() or steam_id64()
    if not sid:
        print("[aviso] SteamID64 não encontrado; pulando biblioteca completa.",
              file=sys.stderr)
        return []
    out: list[dict] = []
    for og in steam_owned(key, sid):
        appid = str(og.get("appid", "")).strip()
        name = str(og.get("name", "")).strip()
        if not appid or appid in installed_ids or appid in STEAM_TOOL_IDS:
            continue
        if any(w in name.lower() for w in STEAM_TOOL_WORDS):
            continue
        art = steam_art(appid)  # usa cache local se existir
        out.append({
            "id": f"steam:{appid}",
            "title": name or f"App {appid}",
            "launcher": "steam",
            "launch_cmd": ["steam", f"steam://rungameid/{appid}"],
            "installed": False,
            "cover": art["cover"] or f"{STEAM_CDN}/{appid}/library_600x900.jpg",
            "hero": art["hero"] or f"{STEAM_CDN}/{appid}/library_hero.jpg",
            "logo": art["logo"] or f"{STEAM_CDN}/{appid}/logo.png",
        })
    return out


# --------------------------------------------------------------------------- #
# Epic via Legendary (CLI próprio em ~/.config/arcadia/runners) — preferencial.
# Heroic (Epic/GOG/Amazon) continua como fallback. Bibliotecas vazias => nada.
# --------------------------------------------------------------------------- #
LEGENDARY_BIN = HOME / ".config/arcadia/runners/legendary"
HEROIC_CACHE = HOME / ".config/heroic/store_cache"
HEROIC_RUNNERS = {"gog": "gog", "legendary": "legendary", "nile": "nile"}


def index_legendary() -> list[dict]:
    """Biblioteca Epic via CLI do Legendary (list-games/list-installed --json)."""
    import subprocess
    if not LEGENDARY_BIN.exists():
        return []
    try:
        r = subprocess.run([str(LEGENDARY_BIN), "list-games", "--json"],
                           capture_output=True, text=True, timeout=60)
        if r.returncode != 0:
            return []
        games = json.loads(r.stdout)
    except Exception:
        return []
    installed: set[str] = set()
    try:
        ri = subprocess.run([str(LEGENDARY_BIN), "list-installed", "--json"],
                            capture_output=True, text=True, timeout=60)
        if ri.returncode == 0:
            installed = {g.get("app_name", "") for g in json.loads(ri.stdout)}
    except Exception:
        pass

    def img(game, *types):
        for t in types:
            for i in (game.get("metadata", {}).get("keyImages") or []):
                if i.get("type") == t and i.get("url"):
                    return i["url"]
        return ""

    out = []
    for g in games:
        app_name = str(g.get("app_name") or "").strip()
        title = str(g.get("app_title") or "").strip()
        if not app_name or not title:
            continue
        out.append({
            "id": f"epic:{app_name}",
            "title": title,
            "launcher": "epic",
            "launch_cmd": [str(LEGENDARY_BIN), "launch", app_name],
            "installed": app_name in installed,
            "cover": img(g, "DieselGameBoxTall", "OfferImageTall"),
            "hero": img(g, "DieselGameBox", "OfferImageWide", "VaultClosed"),
            "logo": img(g, "DieselGameBoxLogo"),
        })
    return out


def _heroic_games_list(data) -> list:
    if isinstance(data, list):
        return data
    if isinstance(data, dict):
        for key in ("library", "games", "data"):
            if isinstance(data.get(key), list):
                return data[key]
    return []


def index_heroic() -> list[dict]:
    games: list[dict] = []
    for fname, runner in HEROIC_RUNNERS.items():
        text = _read(HEROIC_CACHE / f"{fname}_library.json")
        if not text:
            continue
        try:
            data = json.loads(text)
        except json.JSONDecodeError:
            continue
        for g in _heroic_games_list(data):
            if not isinstance(g, dict):
                continue
            app_name = str(g.get("app_name") or g.get("appName") or "").strip()
            title = str(g.get("title") or "").strip()
            if not app_name:
                continue
            # Inclui TODA a biblioteca (possuídos, mesmo não instalados).
            installed = bool(g.get("is_installed"))
            art = g.get("art_cover") or g.get("art_square") or ""
            hero = g.get("art_background") or g.get("art_square") or g.get("art_cover") or ""
            logo = g.get("art_logo") or ""
            games.append({
                "id": f"heroic:{runner}:{app_name}",
                "title": title or app_name,
                "launcher": "heroic",
                "launch_cmd": ["xdg-open", f"heroic://launch/{runner}/{app_name}"],
                "installed": installed,
                "cover": art,   # pode ser URL http(s) do CDN da Epic
                "hero": hero,
                "logo": logo,
            })
    return games


def index_epic_and_heroic() -> list[dict]:
    """Combina Epic (via Legendary CLI, preferencial) com GOG/Amazon (via Heroic).

    index_heroic() também inclui entradas Epic (runner "legendary") lidas do
    cache do Heroic; usamos essas apenas como fallback quando o CLI do
    Legendary não retornar nada, para não perder GOG/Amazon quando há Epic.
    """
    epic = index_legendary()
    heroic = index_heroic()
    non_epic_heroic = [g for g in heroic if not g["id"].startswith("heroic:legendary:")]
    if epic:
        return epic + non_epic_heroic
    epic_via_heroic = [g for g in heroic if g["id"].startswith("heroic:legendary:")]
    return epic_via_heroic + non_epic_heroic


# --------------------------------------------------------------------------- #
# Lutris (SQLite pga.db). Só instalados.
# --------------------------------------------------------------------------- #
LUTRIS_DB = HOME / ".local/share/lutris/pga.db"
LUTRIS_COVER = HOME / ".local/share/lutris/coverart"
LUTRIS_BANNER = HOME / ".cache/lutris/coverart"


def lutris_art(slug: str) -> dict:
    for base in (LUTRIS_COVER, LUTRIS_BANNER):
        for ext in (".jpg", ".png", ".jpeg"):
            p = base / f"{slug}{ext}"
            if p.exists():
                return {"cover": str(p), "hero": "", "logo": ""}
    return {"cover": "", "hero": "", "logo": ""}


def index_lutris(steam_appids: set[str] | None = None) -> list[dict]:
    steam_appids = steam_appids or set()
    if not LUTRIS_DB.exists():
        return []
    games: list[dict] = []
    try:
        with sqlite3.connect(f"file:{LUTRIS_DB}?mode=ro", uri=True) as con:
            rows = con.execute(
                "SELECT id, name, slug, runner, installed, service, service_id "
                "FROM games WHERE installed = 1"
            ).fetchall()
    except sqlite3.Error:
        return []
    for gid, name, slug, runner, _inst, service, service_id in rows:
        # Evita duplicar um jogo Steam que também está catalogado no Lutris.
        if service == "steam" and str(service_id) in steam_appids:
            continue
        slug = slug or ""
        games.append({
            "id": f"lutris:{gid}",
            "title": name or slug,
            "launcher": "lutris",
            "launch_cmd": ["lutris", f"lutris:rungameid/{gid}"],
            "installed": True,
            **lutris_art(slug),
        })
    return games


# --------------------------------------------------------------------------- #
def slssteam_appids(config_path: Path | None = None) -> list[str]:
    """AppIds injetados pelo SLSsteam (bloco AdditionalApps do config.yaml)."""
    text = _read(config_path or SLS_CONFIG)
    if not text:
        return []
    ids: list[str] = []
    in_block = False
    for line in text.splitlines():
        if re.match(r"^AdditionalApps\s*:", line):
            in_block = True
            continue
        if in_block:
            # Sai do bloco ao chegar em outra chave de topo (sem indentação).
            if line and not line[0].isspace() and ":" in line \
                    and not line.lstrip().startswith("#"):
                break
            m = re.match(r"^\s*-\s*(\d+)", line)
            if m:
                ids.append(m.group(1))
    return ids


def index_slssteam(existing_appids: set[str],
                   config_path: Path | None = None) -> list[dict]:
    """Jogos injetados pelo SLSsteam que ainda não estão na biblioteca."""
    libdirs = steam_library_dirs()
    out: list[dict] = []
    seen: set[str] = set()
    for appid in slssteam_appids(config_path):
        if appid in existing_appids or appid in STEAM_TOOL_IDS or appid in seen:
            continue
        seen.add(appid)
        art = steam_art(appid)
        installed = any((d / f"appmanifest_{appid}.acf").exists() for d in libdirs)
        out.append({
            "id": f"steam:{appid}",
            "title": f"App {appid}",  # enrich_steam preenche o nome real
            "launcher": "steam",
            "launch_cmd": ["steam", f"steam://rungameid/{appid}"],
            "installed": installed,
            "cover": art["cover"] or f"{STEAM_CDN}/{appid}/library_600x900.jpg",
            "hero": art["hero"] or f"{STEAM_CDN}/{appid}/library_hero.jpg",
            "logo": art["logo"] or f"{STEAM_CDN}/{appid}/logo.png",
        })
    return out


def main() -> int:
    OUT_DIR.mkdir(parents=True, exist_ok=True)
    library: list[dict] = []
    counts = {}

    # Toggles de fontes (padrão: todas ligadas) e caminho custom do SLSsteam.
    cfg = load_config()
    sources = cfg.get("sources") or {}

    def on(name: str) -> bool:
        return sources.get(name, True) is not False

    sls_path = str(cfg.get("slssteam_path") or "").strip()
    sls_config = Path(sls_path).expanduser() if sls_path else None

    # Steam primeiro, pra desduplicar jogos Steam catalogados no Lutris.
    try:
        steam_games = index_steam() if on("steam") else []
    except Exception as exc:
        print(f"[aviso] steam: {exc}", file=sys.stderr)
        steam_games = []
    steam_appids = {g["id"].split(":", 1)[1] for g in steam_games}

    # Biblioteca completa da Steam (possuídos não instalados) via Web API.
    if on("steam"):
        steam_games = steam_games + steam_owned_games(steam_appids)

    # Jogos injetados pelo SLSsteam (AdditionalApps), que a Web API não retorna.
    if on("slssteam"):
        all_steam_ids = {g["id"].split(":", 1)[1] for g in steam_games}
        steam_games = steam_games + index_slssteam(all_steam_ids, sls_config)

    # Metadados (descrição/gênero/ano/nota) via Steam Store, cacheado.
    # Sem passar o idioma: quem traduz o código do config ("pt-BR") para o
    # nome que a Steam entende ("portuguese") é o _get_steam_lang(). Passando
    # o código cru, o `l=pt-BR` era ignorado pela API e a descrição vinha em
    # inglês mesmo com o app em português.
    try:
        enrich_steam(steam_games, (cfg.get("steamgriddb_api_key") or "").strip())
    except Exception as exc:
        print(f"[aviso] steam-meta: {exc}", file=sys.stderr)

    # Dados do JOGADOR (tempo de jogo) via Web API, cache 24h.
    try:
        enrich_player(steam_games, cfg)
    except Exception as exc:
        print(f"[aviso] player: {exc}", file=sys.stderr)

    # Recomputa após owned+slssteam pra Lutris deduplicar contra biblioteca completa.
    all_steam_appids = {g["id"].split(":", 1)[1] for g in steam_games}
    for name, fn in (
        ("steam", lambda: steam_games),
        # Epic: preferência pelo Legendary próprio; GOG/Amazon sempre via Heroic.
        ("heroic", (index_epic_and_heroic if on("heroic") else (lambda: []))),
        ("lutris", (lambda: index_lutris(all_steam_appids)) if on("lutris") else (lambda: [])),
    ):
        try:
            items = fn()
        except Exception as exc:  # nunca deixa um launcher derrubar o resto
            print(f"[aviso] {name}: {exc}", file=sys.stderr)
            items = []
        counts[name] = len(items)
        library.extend(items)

    library.sort(key=lambda g: g["title"].lower())
    if not _atomic_write(OUT_FILE, json.dumps(library, ensure_ascii=False, indent=2)):
        print(f"[erro] falha ao escrever {OUT_FILE}", file=sys.stderr)
        return 1
    total = len(library)
    print(f"library.json gerado: {total} jogos "
          f"(steam={counts['steam']}, heroic={counts['heroic']}, "
          f"lutris={counts['lutris']}) -> {OUT_FILE}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
