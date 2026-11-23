# RPC torrent do Arcadia: JSON por linha no stdio (request/response por id).
# Pai (Electron) manda {"id": N, "method": ..., "params": {...}} no stdin e
# recebe {"id": N, "result": ...} ou {"id": N, "error": {code, message}}.
#
# Métodos:
#   action { action: "start"|"pause"|"cancel"|"set_download_limit",
#            game_id, url (magnet), save_path, file_indices?, trackers?,
#            max_download_speed_bytes_per_second? }
#   status -> { "<game_id>": {...status} } de todos os downloads ativos
#   torrent_files { magnet, timeout_ms? } -> { name, totalSize, files[] }
#
# Adaptado do python_rpc do Hydra Launcher (GPL-3.0), simplificado:
# sem seeding, sem senha, sem bootstrap por CLI (resume fica no lado Node).
import json
import logging
import re
import sys
import threading
import time
import urllib.parse

import libtorrent as lt

from torrent_downloader import TorrentDownloader

for _stream in (sys.stdin, sys.stdout, sys.stderr):
    reconfigure = getattr(_stream, "reconfigure", None)
    if callable(reconfigure):
        try:
            reconfigure(encoding="utf-8", errors="strict")
        except (ValueError, OSError):
            pass

logging.basicConfig(
    level=logging.INFO,
    format="[%(asctime)s] %(levelname)s %(name)s: %(message)s",
    stream=sys.stderr,
)
logger = logging.getLogger("arcadia.rpc")

torrent_port = sys.argv[1] if len(sys.argv) > 1 else "6881"

downloads = {}
downloads_lock = threading.RLock()
metadata_semaphore = threading.BoundedSemaphore(value=2)
current_download_limit = None

torrent_session = lt.session(
    {
        "listen_interfaces": "0.0.0.0:{port}".format(port=torrent_port),
        # libtorrent 2.x gerencia o cache de disco sozinho (mmap) — a opção
        # cache_size do 1.x foi removida e derruba a sessão se passada.
        "alert_queue_size": 1000,
    }
)

MAGNET_HASH_HEX_RE = re.compile(r"^[a-fA-F0-9]{40}$")
MAGNET_HASH_BASE32_RE = re.compile(r"^[a-zA-Z2-7]{32}$")

TORRENT_FILES_CACHE_TTL = 300
TORRENT_FILES_CACHE_MAX = 64
torrent_files_cache = {}
stdout_lock = threading.RLock()


class RpcError(Exception):
    def __init__(self, code, message=None):
        super().__init__(message or code)
        self.code = code
        self.message = message or code


def validate_magnet_uri(magnet):
    if not isinstance(magnet, str):
        raise ValueError("invalid_magnet")
    magnet = magnet.strip()
    if not magnet.startswith("magnet:") or len(magnet) > 8192:
        raise ValueError("invalid_magnet")
    query = urllib.parse.parse_qs(urllib.parse.urlparse(magnet).query)
    for xt in query.get("xt") or []:
        if not xt.startswith("urn:btih:"):
            continue
        candidate = xt[len("urn:btih:"):].strip()
        if MAGNET_HASH_HEX_RE.match(candidate) or MAGNET_HASH_BASE32_RE.match(candidate):
            return magnet, candidate.lower()
    raise ValueError("invalid_magnet")


def validate_trackers(trackers):
    if trackers is None:
        return []
    if not isinstance(trackers, list):
        raise RpcError("invalid_trackers")
    for t in trackers:
        if not isinstance(t, str):
            raise RpcError("invalid_trackers")
        parsed = urllib.parse.urlparse(t)
        if parsed.scheme not in {"http", "https", "udp", "ws", "wss"} or not parsed.netloc:
            raise RpcError("invalid_trackers")
    return trackers


def parse_file_indices(file_indices):
    if file_indices is None:
        return None
    if not isinstance(file_indices, list):
        raise ValueError("invalid_file_indices")
    for i in file_indices:
        if isinstance(i, bool) or not isinstance(i, int):
            raise ValueError("invalid_file_indices")
    return file_indices


def map_error_code(error):
    code = str(error)
    if isinstance(error, TimeoutError) or code == "metadata_timeout":
        return "metadata_timeout"
    if code in {
        "invalid_magnet", "invalid_file_indices", "empty_selection",
        "invalid_url", "invalid_save_path", "invalid_trackers",
        "metadata_incomplete", "too_many_files",
    }:
        return code
    logger.error("erro RPC não mapeado: %s", error, exc_info=True)
    return "internal_error"


def apply_download_limit(downloader):
    if downloader:
        downloader.set_download_limit(current_download_limit)


def start_torrent_download(game_id, url, save_path, file_indices=None, trackers=None):
    with downloads_lock:
        existing = downloads.get(game_id)

    if existing and isinstance(existing, TorrentDownloader):
        apply_download_limit(existing)
        existing.start_download(url, save_path, file_indices=file_indices, trackers=trackers)
        return

    downloader = TorrentDownloader(torrent_session, session_lock=downloads_lock)
    apply_download_limit(downloader)

    with downloads_lock:
        downloads[game_id] = downloader

    try:
        downloader.start_download(url, save_path, file_indices=file_indices, trackers=trackers)
    except Exception:
        with downloads_lock:
            downloads.pop(game_id, None)
        raise


def action(data):
    global current_download_limit
    action_name = data.get("action")
    game_id = data.get("game_id")
    if not action_name:
        raise RpcError("invalid_action")

    if action_name == "start":
        if not game_id:
            raise RpcError("invalid_game_id")
        url = data.get("url")
        save_path = data.get("save_path")
        if not isinstance(url, str) or not url.startswith("magnet"):
            raise RpcError("invalid_url")
        if not isinstance(save_path, str) or not save_path:
            raise RpcError("invalid_save_path")
        validate_magnet_uri(url)
        # 90s: sessão fria precisa bootstrappar DHT antes de resolver metadata.
        start_torrent_download(
            game_id,
            url,
            save_path,
            file_indices=parse_file_indices(data.get("file_indices")),
            trackers=validate_trackers(data.get("trackers")),
        )
    elif action_name in ("pause", "cancel"):
        with downloads_lock:
            downloader = downloads.pop(game_id, None) if action_name == "cancel" else downloads.get(game_id)
        if downloader:
            downloader.pause_download() if action_name == "pause" else downloader.cancel_download()
    elif action_name == "set_download_limit":
        try:
            value = int(data.get("max_download_speed_bytes_per_second"))
        except (TypeError, ValueError):
            value = None
        current_download_limit = value if value and value > 0 else None
        with downloads_lock:
            ativos = list(downloads.values())
        for d in ativos:
            apply_download_limit(d)
    else:
        raise RpcError("invalid_action")


def status(_data):
    with downloads_lock:
        items = list(downloads.items())
    out = {}
    for game_id, downloader in items:
        s = downloader.get_download_status() if downloader else None
        if s:
            out[str(game_id)] = s
    return out


def torrent_files(data):
    import tempfile

    try:
        magnet, info_hash = validate_magnet_uri(data.get("magnet"))
    except Exception as error:
        raise RpcError(map_error_code(error)) from error

    cached = torrent_files_cache.get(info_hash)
    if cached and time.time() - cached["at"] <= TORRENT_FILES_CACHE_TTL:
        return cached["value"]

    try:
        timeout_ms = int(data.get("timeout_ms", 60000))
    except (TypeError, ValueError):
        timeout_ms = 60000
    timeout_seconds = max(5000, min(timeout_ms, 120000)) / 1000

    if not metadata_semaphore.acquire(timeout=5):
        raise RpcError("metadata_busy")

    temp = TorrentDownloader(torrent_session, session_lock=downloads_lock)
    try:
        temp.start_download(magnet, tempfile.gettempdir(), trackers=validate_trackers(data.get("trackers")))
        payload = {"infoHash": info_hash, **temp.get_torrent_files(timeout_seconds=timeout_seconds)}
        if len(torrent_files_cache) >= TORRENT_FILES_CACHE_MAX:
            torrent_files_cache.pop(min(torrent_files_cache, key=lambda k: torrent_files_cache[k]["at"]), None)
        torrent_files_cache[info_hash] = {"at": time.time(), "value": payload}
        return payload
    except Exception as error:
        raise RpcError(map_error_code(error)) from error
    finally:
        temp.cancel_download()
        metadata_semaphore.release()


METHODS = {"action": action, "status": status, "torrent_files": torrent_files}


def write_response(payload):
    with stdout_lock:
        sys.stdout.write(json.dumps(payload, ensure_ascii=True, separators=(",", ":")) + "\n")
        sys.stdout.flush()


def handle_request(request):
    request_id = request.get("id")
    method = request.get("method")
    params = request.get("params") or {}
    try:
        fn = METHODS.get(method)
        if not fn:
            raise RpcError("method_not_found")
        if not isinstance(params, dict):
            raise RpcError("invalid_params")
        write_response({"id": request_id, "result": fn(params)})
    except RpcError as error:
        write_response({"id": request_id, "error": {"code": error.code, "message": error.message}})
    except Exception as error:
        write_response({"id": request_id, "error": {"code": map_error_code(error), "message": str(error)}})


def main():
    write_response({"event": "ready", "protocolVersion": 1})
    for raw_line in sys.stdin:
        line = raw_line.strip()
        if not line:
            continue
        try:
            payload = json.loads(line)
        except Exception:
            write_response({"id": None, "error": {"code": "invalid_json", "message": "invalid_json"}})
            continue
        if isinstance(payload, dict):
            threading.Thread(target=handle_request, args=(payload,), daemon=True).start()


if __name__ == "__main__":
    main()
