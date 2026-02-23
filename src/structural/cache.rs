//! Incremental caching for structural analysis.
//!
//! Stores file mtimes and cached analysis results to avoid
//! re-parsing unchanged files.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use serde::{Deserialize, Serialize};

/// Cache for file analysis results.
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct AnalysisCache {
    /// Map of file path -> cached entry
    entries: HashMap<PathBuf, CacheEntry>,
    /// Version of the cache format
    version: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CacheEntry {
    /// File modification time (as duration since UNIX_EPOCH)
    pub mtime_secs: u64,
    /// File size in bytes
    pub size: u64,
    /// Module names declared in this file
    pub modules: Vec<String>,
    /// Import paths
    pub imports: Vec<CachedImport>,
    /// Type references
    pub type_refs: Vec<String>,
    /// Trait implementations
    pub trait_impls: Vec<CachedTraitImpl>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CachedImport {
    pub path: String,
    pub names: Vec<String>,
    pub line: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CachedTraitImpl {
    pub type_name: String,
    pub trait_name: String,
}

const CACHE_VERSION: u32 = 1;
const CACHE_FILENAME: &str = ".intent-cache.json";

impl AnalysisCache {
    pub fn new() -> Self {
        Self {
            entries: HashMap::new(),
            version: CACHE_VERSION,
        }
    }

    /// Load cache from disk, returning empty cache on failure.
    pub fn load(codebase: &Path) -> Self {
        let cache_path = codebase.join(CACHE_FILENAME);
        match std::fs::read_to_string(&cache_path) {
            Ok(content) => {
                match serde_json::from_str::<AnalysisCache>(&content) {
                    Ok(cache) if cache.version == CACHE_VERSION => cache,
                    _ => Self::new(),
                }
            }
            Err(_) => Self::new(),
        }
    }

    /// Save cache to disk.
    pub fn save(&self, codebase: &Path) -> std::io::Result<()> {
        let cache_path = codebase.join(CACHE_FILENAME);
        let content = serde_json::to_string(self)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
        std::fs::write(&cache_path, content)
    }

    /// Check if a file's cached entry is still valid.
    pub fn is_valid(&self, path: &Path) -> bool {
        if let Some(entry) = self.entries.get(path) {
            if let Ok(metadata) = std::fs::metadata(path) {
                if let Ok(mtime) = metadata.modified() {
                    if let Ok(duration) = mtime.duration_since(SystemTime::UNIX_EPOCH) {
                        return entry.mtime_secs == duration.as_secs()
                            && entry.size == metadata.len();
                    }
                }
            }
        }
        false
    }

    /// Get cached entry for a file.
    pub fn get(&self, path: &Path) -> Option<&CacheEntry> {
        if self.is_valid(path) {
            self.entries.get(path)
        } else {
            None
        }
    }

    /// Update cache entry for a file.
    pub fn update(&mut self, path: PathBuf, entry: CacheEntry) {
        self.entries.insert(path, entry);
    }

    /// Remove stale entries for files that no longer exist.
    pub fn prune(&mut self) {
        self.entries.retain(|path, _| path.exists());
    }

    /// Get the number of cached entries.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Check if cache is empty.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}
