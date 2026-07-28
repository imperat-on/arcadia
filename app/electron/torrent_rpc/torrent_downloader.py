# Cliente torrent do Arcadia (libtorrent-rasterbar).
#
# Adaptado do python_rpc do Hydra Launcher (GPL-3.0):
# https://github.com/hydralauncher/hydra — ver NOTICE no README do projeto.
# Simplificações: sem seeding dedicado, sem bootstrap por CLI, sem senha RPC
# (stdio só é acessível pelo processo pai).
import logging
import threading
import time
from typing import List, Optional, Set

import libtorrent as lt


class TorrentDownloader:
    def __init__(
        self,
        torrent_session,
        flags=lt.torrent_flags.auto_managed,
        session_lock: Optional[threading.RLock] = None,
    ):
        self.torrent_handle = None
        self.session = torrent_session
        self.flags = flags
        self.session_lock = session_lock or threading.RLock()
        self.selected_file_indices = None
        self.selected_size_bytes = None
        self.logger = logging.getLogger("arcadia.torrent")
        # Trackers públicos de fallback (fontes raramente embutem trackers bons).
        self.trackers = [
            "udp://tracker.opentrackr.org:1337/announce",
            "udp://open.tracker.cl:1337/announce",
            "udp://open.demonii.com:1337/announce",
            "udp://open.stealth.si:80/announce",
            "udp://tracker.torrent.eu.org:451/announce",
            "udp://exodus.desync.com:6969/announce",
            "udp://tracker.theoks.net:6969/announce",
            "udp://explodie.org:6969/announce",
            "udp://tracker2.dler.org:80/announce",
            "udp://tracker.dler.org:6969/announce",
            "udp://retracker01-msk-virt.corbina.net:80/announce",
            "udp://opentracker.io:6969/announce",
            "udp://new-line.net:6969/announce",
            "udp://moonburrow.club:6969/announce",
            "udp://leet-tracker.moe:1337/announce",
            "udp://bt2.archive.org:6969/announce",
            "udp://bt1.archive.org:6969/announce",
            "http://tracker2.dler.org:80/announce",
            "http://tracker.dler.org:6969/announce",
        ]

    def set_download_limit(self, max_download_speed: int = None):
        download_limit = (
            max_download_speed if max_download_speed and max_download_speed > 0 else 0
        )
        try:
            self.session.apply_settings({"download_rate_limit": download_limit})
        except Exception:
            self.logger.warning("falha ao aplicar limite de download", exc_info=True)

    def _build_add_torrent_params(self, magnet, save_path, flags, trackers=None):
        try:
            params = lt.parse_magnet_uri(magnet)
        except Exception as error:
            raise ValueError("invalid_magnet") from error

        params.save_path = save_path
        params.flags = params.flags | flags

        extra = trackers or []
        known = set(params.trackers)
        tiers = list(params.tracker_tiers)[: len(params.trackers)]
        tiers.extend([0] * (len(params.trackers) - len(tiers)))

        for t in extra:
            if t not in known:
                params.trackers.append(t)
                known.add(t)
                tiers.append(0)

        fallback_tier = max(tiers) + 1 if tiers else 0
        for t in self.trackers:
            if t not in known:
                params.trackers.append(t)
                known.add(t)
                tiers.append(fallback_tier)

        params.tracker_tiers = tiers
        return params

    def _get_torrent_info(self):
        if not self.torrent_handle or not self.torrent_handle.is_valid():
            return None
        getter = getattr(self.torrent_handle, "torrent_file", None) or getattr(
            self.torrent_handle, "get_torrent_info", None
        )
        if not callable(getter):
            return None
        try:
            return getter()
        except RuntimeError:
            return None

    def _wait_for_metadata(self, timeout_seconds: float = 30.0):
        if not self.torrent_handle or not self.torrent_handle.is_valid():
            return False
        deadline = time.monotonic() + max(timeout_seconds, 1.0)
        while time.monotonic() < deadline:
            try:
                status = self.torrent_handle.status()
            except RuntimeError:
                return False
            if status.has_metadata:
                return True
            time.sleep(0.25)
        return False

    def _sanitize_file_indices(self, file_indices, files_storage):
        if file_indices is None:
            return None
        if not isinstance(file_indices, list):
            raise ValueError("invalid_file_indices")
        max_index = files_storage.num_files() - 1
        sanitized: Set[int] = set()
        for index in file_indices:
            if isinstance(index, bool) or not isinstance(index, int):
                raise ValueError("invalid_file_indices")
            if index < 0 or index > max_index:
                raise ValueError("invalid_file_indices")
            sanitized.add(index)
        if not sanitized:
            raise ValueError("empty_selection")
        return sorted(sanitized)

    def _set_selected_file_priorities(self, selected_indices, files_storage):
        priorities = [0] * files_storage.num_files()
        for index in selected_indices:
            priorities[index] = 1
        self.torrent_handle.prioritize_files(priorities)

    def start_download(
        self,
        magnet: str,
        save_path: str,
        file_indices: Optional[List[int]] = None,
        wait_timeout_seconds: float = 90.0,
        trackers: Optional[List[str]] = None,
    ):
        selective = file_indices is not None

        with self.session_lock:
            if self.torrent_handle and self.torrent_handle.is_valid():
                # Mesmo torrent já adicionado (resume após restart do app):
                # o libtorrent verifica os arquivos no disco e continua.
                if not selective:
                    self.torrent_handle.set_flags(lt.torrent_flags.auto_managed)
                    self.torrent_handle.resume()
                    return
                self.torrent_handle.pause()
                self.session.remove_torrent(self.torrent_handle)
                self.torrent_handle = None

            initial_flags = self.flags | lt.torrent_flags.paused
            if selective:
                initial_flags |= lt.torrent_flags.default_dont_download
            initial_flags |= lt.torrent_flags.auto_managed

            params = self._build_add_torrent_params(magnet, save_path, initial_flags, trackers)
            self.torrent_handle = self.session.add_torrent(params)

        self.selected_file_indices = None
        self.selected_size_bytes = None

        if selective:
            try:
                self.torrent_handle.resume()
                if not self._wait_for_metadata(timeout_seconds=wait_timeout_seconds):
                    raise TimeoutError("metadata_timeout")
                info = self._get_torrent_info()
                if info is None:
                    raise RuntimeError("metadata_incomplete")
                files_storage = info.files()
                self.torrent_handle.pause()
                sanitized = self._sanitize_file_indices(file_indices, files_storage)
                self._set_selected_file_priorities(sanitized, files_storage)
                self.selected_file_indices = sanitized
                self.selected_size_bytes = sum(
                    files_storage.file_size(i) for i in sanitized
                )
            except Exception:
                self.cancel_download()
                raise

        self.torrent_handle.set_flags(lt.torrent_flags.auto_managed)
        self.torrent_handle.resume()

    def get_torrent_files(self, timeout_seconds: float = 60.0, max_files: int = 100000):
        if not self._wait_for_metadata(timeout_seconds=timeout_seconds):
            raise TimeoutError("metadata_timeout")
        info = self._get_torrent_info()
        if info is None:
            raise RuntimeError("metadata_incomplete")
        files_storage = info.files()
        count = files_storage.num_files()
        if count > max_files:
            raise OverflowError("too_many_files")
        return {
            "name": info.name(),
            "totalSize": info.total_size(),
            "files": [
                {
                    "index": i,
                    "path": files_storage.file_path(i),
                    "length": files_storage.file_size(i),
                }
                for i in range(count)
            ],
        }

    def pause_download(self):
        if self.torrent_handle:
            self.torrent_handle.pause()
            self.torrent_handle.unset_flags(lt.torrent_flags.auto_managed)

    def cancel_download(self):
        with self.session_lock:
            if self.torrent_handle:
                if self.torrent_handle.is_valid():
                    self.torrent_handle.pause()
                    self.session.remove_torrent(
                        self.torrent_handle, lt.session.delete_partfile
                    )
                self.torrent_handle = None
                self.selected_file_indices = None
                self.selected_size_bytes = None

    def abort_session(self):
        self.cancel_download()
        abort = getattr(self.session, "abort", None)
        if callable(abort):
            abort()

    def get_download_status(self):
        if not self.torrent_handle or not self.torrent_handle.is_valid():
            return None
        try:
            status = self.torrent_handle.status()
        except RuntimeError:
            return None

        info = self._get_torrent_info() if status.has_metadata else None
        file_size = getattr(status, "total_wanted", 0) or (
            self.selected_size_bytes or (info.total_size() if info else 0)
        )
        bytes_downloaded = getattr(status, "total_wanted_done", -1)
        if bytes_downloaded < 0:
            bytes_downloaded = (
                int(status.progress * file_size) if file_size > 0 else status.all_time_download
            )
        progress = min(max(bytes_downloaded / file_size, 0), 1) if file_size > 0 else status.progress

        return {
            "folderName": info.name() if info else "",
            "fileSize": file_size,
            "progress": progress,
            "downloadSpeed": status.download_rate,
            "uploadSpeed": status.upload_rate,
            "numPeers": status.num_peers,
            "numSeeds": status.num_seeds,
            "state": str(status.state),
            "bytesDownloaded": bytes_downloaded,
        }
