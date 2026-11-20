//! Bounded page-bitmap cache.
//!
//! Rendered pages are expensive (a 1× A4 page is ~2 MB of pixels), so the cache
//! is capped by *bytes*, not entries, and evicts least-recently-used pages when
//! it overflows. Rendering a different zoom level reuses the closest cached
//! scale as a placeholder, which keeps zooming smooth while the exact bitmap is
//! being produced on the worker thread.

use std::collections::HashMap;
use std::sync::Arc;

use crate::pdf::RenderKey;
use crate::ui::Bitmap;

struct Entry {
    bitmap: Arc<Bitmap>,
    bytes: usize,
    last_used: u64,
}

pub struct PageCache {
    entries: HashMap<RenderKey, Entry>,
    bytes: usize,
    limit_bytes: usize,
    clock: u64,
    pub hits: u64,
    pub misses: u64,
}

impl PageCache {
    pub fn new(limit_bytes: usize) -> PageCache {
        PageCache {
            entries: HashMap::new(),
            bytes: 0,
            limit_bytes,
            clock: 1,
            hits: 0,
            misses: 0,
        }
    }

    pub fn set_limit(&mut self, limit_bytes: usize) {
        self.limit_bytes = limit_bytes.max(32 * 1024 * 1024);
        self.evict();
    }

    pub fn memory_bytes(&self) -> usize {
        self.bytes
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn contains(&self, key: &RenderKey) -> bool {
        self.entries.contains_key(key)
    }

    /// Looks a bitmap up and marks it as recently used.
    pub fn get(&mut self, key: &RenderKey) -> Option<Arc<Bitmap>> {
        self.clock += 1;
        match self.entries.get_mut(key) {
            Some(entry) => {
                entry.last_used = self.clock;
                self.hits += 1;
                Some(entry.bitmap.clone())
            }
            None => {
                self.misses += 1;
                None
            }
        }
    }

    pub fn peek(&self, key: &RenderKey) -> Option<Arc<Bitmap>> {
        self.entries.get(key).map(|e| e.bitmap.clone())
    }

    pub fn insert(&mut self, key: RenderKey, bitmap: Arc<Bitmap>) {
        let bytes = bitmap.px.len() * 4;
        self.clock += 1;
        if let Some(old) = self.entries.insert(
            key,
            Entry {
                bitmap,
                bytes,
                last_used: self.clock,
            },
        ) {
            self.bytes = self.bytes.saturating_sub(old.bytes);
        }
        self.bytes += bytes;
        self.evict();
    }

    /// Best available bitmap for `page` (closest zoom, same rotation) — used as
    /// a placeholder until the exact scale arrives.
    pub fn best_for(&self, page: u32, rotation: u16) -> Option<(RenderKey, Arc<Bitmap>)> {
        let mut best: Option<(u32, RenderKey, Arc<Bitmap>)> = None;
        for (key, entry) in &self.entries {
            if key.page != page || key.rotation != rotation {
                continue;
            }
            match &best {
                Some((dist, _, _)) if *dist <= key.scale_milli.abs_diff(0) => {}
                _ => {}
            }
            // Prefer the smallest zoom that is still >= the caller's request is
            // not known here, so simply take the largest bitmap (best quality).
            let score = key.scale_milli;
            if best.as_ref().map(|(s, _, _)| score > *s).unwrap_or(true) {
                best = Some((score, *key, entry.bitmap.clone()));
            }
        }
        best.map(|(_, k, b)| (k, b))
    }

    /// Drops cached scales that are far away from `keep_scale` for the pages in
    /// `keep_pages`, to reclaim memory when the user zooms around.
    pub fn prune_offscale(&mut self, keep_pages: &[u32], keep_scale_milli: u32, keep_rotation: u16) {
        let before = self.bytes;
        self.entries.retain(|key, entry| {
            let keep = keep_pages.contains(&key.page)
                && (key.rotation == keep_rotation || key.rotation == 0)
                && key.scale_milli.abs_diff(keep_scale_milli) < keep_scale_milli / 2 + 1;
            if !keep {
                self.bytes -= entry.bytes;
            }
            keep
        });
        let _ = before;
    }

    pub fn clear(&mut self) {
        self.entries.clear();
        self.bytes = 0;
    }

    fn evict(&mut self) {
        while self.bytes > self.limit_bytes && !self.entries.is_empty() {
            let victim = self
                .entries
                .iter()
                .min_by_key(|(_, e)| e.last_used)
                .map(|(k, _)| *k);
            match victim {
                Some(key) => {
                    if let Some(entry) = self.entries.remove(&key) {
                        self.bytes = self.bytes.saturating_sub(entry.bytes);
                    }
                }
                None => break,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::canvas::Color;

    fn bmp(w: u32, h: u32) -> Arc<Bitmap> {
        Arc::new(Bitmap::filled(w, h, Color::rgb(255, 255, 255)))
    }

    #[test]
    fn inserts_and_hits() {
        let mut cache = PageCache::new(64 * 1024 * 1024);
        let key = RenderKey::new(0, 1.0, 0);
        assert!(cache.get(&key).is_none());
        cache.insert(key, bmp(100, 100));
        assert!(cache.get(&key).is_some());
        assert_eq!(cache.hits, 1);
        assert_eq!(cache.misses, 1);
        assert_eq!(cache.memory_bytes(), 100 * 100 * 4);
    }

    #[test]
    fn evicts_least_recently_used_when_over_budget() {
        let mut cache = PageCache::new(32 * 1024 * 1024);
        // Each 1000x1000 bitmap is 4 MB; the limit is 32 MB.
        for page in 0..12u32 {
            let key = RenderKey::new(page, 1.0, 0);
            cache.insert(key, bmp(1000, 1000));
        }
        assert!(cache.memory_bytes() <= 32 * 1024 * 1024);
        assert!(cache.len() < 12);
        // The most recent pages survive.
        assert!(cache.contains(&RenderKey::new(11, 1.0, 0)));
        assert!(!cache.contains(&RenderKey::new(0, 1.0, 0)));
    }

    #[test]
    fn replacement_updates_byte_accounting() {
        let mut cache = PageCache::new(64 * 1024 * 1024);
        let key = RenderKey::new(0, 1.0, 0);
        cache.insert(key, bmp(100, 100));
        cache.insert(key, bmp(200, 200));
        assert_eq!(cache.memory_bytes(), 200 * 200 * 4);
        assert_eq!(cache.len(), 1);
    }

    #[test]
    fn best_for_prefers_the_largest_scale() {
        let mut cache = PageCache::new(64 * 1024 * 1024);
        cache.insert(RenderKey::new(0, 1.0, 0), bmp(100, 100));
        cache.insert(RenderKey::new(0, 2.0, 0), bmp(200, 200));
        cache.insert(RenderKey::new(0, 2.0, 90), bmp(200, 200));
        let (key, bitmap) = cache.best_for(0, 0).unwrap();
        assert_eq!(key.scale_milli, 2000);
        assert_eq!(bitmap.width, 200);
        assert!(cache.best_for(5, 0).is_none());
    }

    #[test]
    fn pruning_keeps_only_the_active_scale() {
        let mut cache = PageCache::new(64 * 1024 * 1024);
        cache.insert(RenderKey::new(0, 1.0, 0), bmp(50, 50));
        cache.insert(RenderKey::new(0, 3.0, 0), bmp(50, 50));
        cache.insert(RenderKey::new(9, 3.0, 0), bmp(50, 50));
        cache.prune_offscale(&[0], 3000, 0);
        assert!(cache.contains(&RenderKey::new(0, 3.0, 0)));
        assert!(!cache.contains(&RenderKey::new(0, 1.0, 0)));
        assert!(!cache.contains(&RenderKey::new(9, 3.0, 0)));
    }
}
