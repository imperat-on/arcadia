//! Cache de bytes de imagens; I/O de disco nunca executa na thread da UI.
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, SystemTime};

use sha2::{Digest, Sha256};

const TTL: Duration = Duration::from_secs(7 * 24 * 60 * 60);
pub const MAX_BYTES: usize = 16 * 1024 * 1024;
const DISK_BUDGET: u64 = 256 * 1024 * 1024;
const PRUNE_BATCH: u64 = 8 * 1024 * 1024;

pub struct AssetCache {
    root: PathBuf,
    // Commits share the same budget. Recount on the first write and after
    // consuming the remaining space, instead of stat'ing every file per image.
    bytes_until_prune: Mutex<u64>,
}

impl AssetCache {
    pub fn new(root: PathBuf) -> Self {
        Self { root, bytes_until_prune: Mutex::new(0) }
    }

    pub fn read(&self, url: &str) -> Option<Vec<u8>> {
        let path = path(&self.root, url);
        let meta = std::fs::metadata(&path).ok()?;
        if meta.len() == 0 || meta.len() > MAX_BYTES as u64
            || meta.modified().ok()?.elapsed().ok()? > TTL {
            return None;
        }
        std::fs::read(path).ok()
    }

    pub fn write(&self, url: &str, bytes: &[u8]) {
        if bytes.is_empty() || bytes.len() > MAX_BYTES {
            return;
        }
        let _ = self.commit(url, bytes);
    }

    fn commit(&self, url: &str, bytes: &[u8]) -> std::io::Result<()> {
        use std::sync::atomic::{AtomicU64, Ordering};
        static SERIAL: AtomicU64 = AtomicU64::new(0);

        std::fs::create_dir_all(&self.root)?;
        let destination = path(&self.root, url);
        let serial = SERIAL.fetch_add(1, Ordering::Relaxed);
        let temporary = destination.with_extension(format!("{}.{serial}.part", std::process::id()));
        let result = std::fs::write(&temporary, bytes).and_then(|()| {
            let mut remaining = self.bytes_until_prune.lock().unwrap_or_else(|err| err.into_inner());
            std::fs::rename(&temporary, &destination)?;
            *remaining = remaining.saturating_sub(bytes.len() as u64);
            if *remaining == 0 {
                *remaining = self.prune().unwrap_or(PRUNE_BATCH);
            }
            Ok(())
        });
        if result.is_err() {
            let _ = std::fs::remove_file(temporary);
        }
        result
    }

    fn prune(&self) -> std::io::Result<u64> {
        let mut files: Vec<_> = std::fs::read_dir(&self.root)?
            .flatten()
            .filter_map(|entry| {
                let path = entry.path();
                if path.extension()?.to_str()? != "image" {
                    return None;
                }
                let meta = entry.metadata().ok()?;
                Some((path, meta.len(), meta.modified().unwrap_or(SystemTime::UNIX_EPOCH)))
            })
            .collect();
        let mut total: u64 = files.iter().map(|(_, bytes, _)| *bytes).sum();
        let target = DISK_BUDGET - PRUNE_BATCH;
        if total > target {
            files.sort_unstable_by_key(|(_, _, at)| *at);
            for (path, bytes, _) in files {
                if total <= target {
                    break;
                }
                if std::fs::remove_file(path).is_ok() {
                    total = total.saturating_sub(bytes);
                }
            }
        }
        Ok(DISK_BUDGET.saturating_sub(total).max(PRUNE_BATCH))
    }
}

pub fn path(root: &Path, url: &str) -> PathBuf {
    root.join(format!("{:x}.image", Sha256::digest(url.as_bytes())))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cached_image_survives_a_new_reader_without_network() {
        let root = std::env::temp_dir().join(format!("arcadia-asset-cache-{}", std::process::id()));
        let url = "https://example.test/art.jpg";
        let cache = AssetCache::new(root.clone());
        assert!(cache.read(url).is_none());
        cache.write(url, b"image fixture");
        assert_eq!(
            AssetCache::new(root.clone()).read(url),
            Some(b"image fixture".to_vec())
        );
        assert_ne!(
            path(&root, url),
            path(&root, "https://example.test/other.jpg")
        );
        let _ = std::fs::remove_dir_all(root);
    }
}
