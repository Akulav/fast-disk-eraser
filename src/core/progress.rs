//! Progress shared between a worker thread and the UI (lock-free counters, cancel flag).

use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering::Relaxed};
use std::sync::Mutex;

#[derive(Default)]
pub struct Progress {
    pub bytes_done: AtomicU64,
    pub bytes_total: AtomicU64,
    pub items_done: AtomicU64,
    pub items_total: AtomicU64,
    pub pass: AtomicU32,
    pub passes: AtomicU32,
    phase: Mutex<String>,
    current: Mutex<String>,
    cancel: AtomicBool,
}

impl Progress {
    pub fn new() -> std::sync::Arc<Self> {
        std::sync::Arc::new(Self::default())
    }

    pub fn add_bytes(&self, n: u64) {
        self.bytes_done.fetch_add(n, Relaxed);
    }

    pub fn item_done(&self) {
        self.items_done.fetch_add(1, Relaxed);
    }

    pub fn set_phase(&self, s: impl Into<String>) {
        *self.phase.lock().unwrap() = s.into();
    }

    pub fn phase(&self) -> String {
        self.phase.lock().unwrap().clone()
    }

    pub fn set_current(&self, s: impl Into<String>) {
        *self.current.lock().unwrap() = s.into();
    }

    pub fn current(&self) -> String {
        self.current.lock().unwrap().clone()
    }

    pub fn cancel(&self) {
        self.cancel.store(true, Relaxed);
    }

    pub fn cancelled(&self) -> bool {
        self.cancel.load(Relaxed)
    }

    /// 0..=1, or `None` while the total isn't known yet.
    pub fn fraction(&self) -> Option<f32> {
        let total = self.bytes_total.load(Relaxed);
        (total > 0).then(|| (self.bytes_done.load(Relaxed) as f64 / total as f64).clamp(0.0, 1.0) as f32)
    }
}
