use adblock::{lists::ParseOptions, request::Request, Engine, FilterSet};
use std::{
    fs,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc, RwLock,
    },
};

use super::filters::{refresh_filter_cache, FILTER_FILES};

#[derive(Debug, Clone, Copy, Default)]
pub struct BlockStats {
    pub checked: u64,
    pub blocked: u64,
    pub allowed: u64,
}

pub struct AdblockState {
    engine: RwLock<Engine>,
    checked: AtomicU64,
    blocked: AtomicU64,
    allowed: AtomicU64,
    enabled: AtomicBool,
}

impl AdblockState {
    pub fn load(cache_dir: PathBuf) -> Arc<Self> {
        let engine = load_cached_engine(&cache_dir);
        let state = Arc::new(Self {
            engine: RwLock::new(engine),
            checked: AtomicU64::new(0),
            blocked: AtomicU64::new(0),
            allowed: AtomicU64::new(0),
            enabled: AtomicBool::new(true),
        });

        // Refresh filter lists off the UI thread. Cached rules remain active
        // while the update is running or if the network is unavailable.
        let refresh_state = Arc::clone(&state);
        std::thread::spawn(move || {
            if refresh_filter_cache(&cache_dir) {
                let replacement = load_cached_engine(&cache_dir);
                if let Ok(mut current) = refresh_state.engine.write() {
                    *current = replacement;
                    eprintln!("[Pake Adblock] Filter lists refreshed.");
                }
            }
        });

        state
    }

    pub fn is_enabled(&self) -> bool {
        self.enabled.load(Ordering::Relaxed)
    }

    pub fn set_enabled(&self, enabled: bool) {
        self.enabled.store(enabled, Ordering::Relaxed);
    }

    pub fn stats(&self) -> BlockStats {
        BlockStats {
            checked: self.checked.load(Ordering::Relaxed),
            blocked: self.blocked.load(Ordering::Relaxed),
            allowed: self.allowed.load(Ordering::Relaxed),
        }
    }

    /// Returns true only when the Brave engine matches a blocking rule.
    pub fn should_block(
        &self,
        request_url: &str,
        source_url: &str,
        request_type: &str,
        method: &str,
    ) -> bool {
        if !self.is_enabled() {
            return false;
        }

        let Ok(request) = Request::new(request_url, source_url, request_type, method) else {
            return false;
        };

        self.checked.fetch_add(1, Ordering::Relaxed);
        let blocked = self
            .engine
            .read()
            .map(|engine| engine.check_network_request(&request).should_block())
            .unwrap_or(false);

        if blocked {
            self.blocked.fetch_add(1, Ordering::Relaxed);
        } else {
            self.allowed.fetch_add(1, Ordering::Relaxed);
        }

        blocked
    }
}

fn load_cached_engine(cache_dir: &Path) -> Engine {
    let mut filter_set = FilterSet::new(false);
    // A small built-in safety net is available even before the first list
    // download completes. Full EasyList/EasyPrivacy rules are loaded from cache.
    filter_set.add_filter_list(
        "||doubleclick.net^\n||googlesyndication.com^\n||googleadservices.com^\n||adservice.google.com^\n".to_string(),
        ParseOptions::default(),
    );
    let mut loaded = 0usize;

    for filename in FILTER_FILES {
        let path = cache_dir.join(filename);
        if let Ok(contents) = fs::read_to_string(path) {
            if !contents.trim().is_empty() {
                filter_set.add_filter_list(contents, ParseOptions::default());
                loaded += 1;
            }
        }
    }

    eprintln!("[Pake Adblock] Loaded {loaded} cached filter list(s).");
    Engine::new_with_filter_set(filter_set)
}
