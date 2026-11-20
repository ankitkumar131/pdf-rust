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
    /// Pages dropped to stay inside the budget (per document, but the global
    /// budget is enforced by [`crate::app::App::enforce_cache_budget`]).
    pub evictions: u64,
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
            evictions: 0,
        }
    }

    pub fn set_limit(&mut self, limit_bytes: usize) {
        self.limit_bytes = limit_bytes.max(8 * 1024 * 1024);
        self.evict();
    }

    pub fn memory_bytes(&self) -> usize {
        self.bytes
    }

    /// The effective byte budget (after clamping), so callers and tests can
    /// assert against the same number the cache enforces.
    pub fn limit_bytes(&self) -> usize {
        self.limit_bytes
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

    /// Keys currently cached, oldest first.
    pub fn pages(&self) -> Vec<u32> {
        let mut keys: Vec<(&RenderKey, &Entry)> = self.entries.iter().collect();
        keys.sort_by_key(|(_, entry)| entry.last_used);
        keys.iter().map(|(key, _)| key.page).collect()
    }

    /// Page of the most recently used entry.
    pub fn newest_page(&self) -> Option<u32> {
        self.entries
            .iter()
            .max_by_key(|(_, entry)| entry.last_used)
            .map(|(key, _)| key.page)
    }

    /// Drops least-recently-used entries until at most `target` bytes are cached.
    ///
    /// Pages listed in `protect` are never dropped (the pages on screen), and at
    /// least one entry is always kept so the visible page cannot disappear.
    /// Returns the number of bytes freed.
    pub fn trim_to(&mut self, target: usize, protect: &[u32]) -> usize {
        let before = self.bytes;
        let mut order: Vec<(u64, RenderKey, usize)> = self
            .entries
            .iter()
            .map(|(key, entry)| (entry.last_used, *key, entry.bytes))
            .collect();
        order.sort_by_key(|(last_used, _, _)| *last_used);
        for (last_used, key, bytes) in order {
            if self.bytes <= target {
                break;
            }
            if self.entries.len() <= 1 || protect.contains(&key.page) {
                continue;
            }
            // Never drop the newest entry either: it is the page being shown.
            if self.newest_last_used() == Some(last_used) {
                continue;
            }
            self.entries.remove(&key);
            self.bytes = self.bytes.saturating_sub(bytes);
            self.evictions += 1;
        }
        before - self.bytes
    }

    fn newest_last_used(&self) -> Option<u64> {
        self.entries.values().map(|entry| entry.last_used).max()
    }

    /// Drops every cached page that is not in `keep` (used when a document goes
    /// to the background).
    pub fn keep_only(&mut self, keep: &[u32]) -> usize {
        let before = self.bytes;
        self.entries.retain(|key, entry| {
            let keep_it = keep.contains(&key.page);
            if !keep_it {
                self.bytes = self.bytes.saturating_sub(entry.bytes);
                self.evictions += 1;
            }
            keep_it
        });
        before - self.bytes
    }

    fn evict(&mut self) {
        // Always keep the most recently used page, even if a single bitmap is
        // bigger than the whole budget: evicting it would make the viewer
        // re-render the page it is showing on every frame.
        while self.bytes > self.limit_bytes && self.entries.len() > 1 {
            let newest = self
                .entries
                .iter()
                .max_by_key(|(_, e)| e.last_used)
                .map(|(k, _)| *k);
            let victim = self
                .entries
                .iter()
                .min_by_key(|(_, e)| e.last_used)
                .map(|(k, _)| *k);
            match victim {
                Some(key) if Some(key) != newest => {
                    if let Some(entry) = self.entries.remove(&key) {
                        self.bytes = self.bytes.saturating_sub(entry.bytes);
                        self.evictions += 1;
                    }
                }
                _ => break,
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
    fn respects_the_budget_and_keeps_the_visible_page() {
        let mut cache = PageCache::new(8 * 1024 * 1024);
        assert_eq!(cache.limit_bytes(), 8 * 1024 * 1024);
        // 1000x700 = 2.8 MB per page, so at most three fit.
        for page in 0..6 {
            cache.insert(RenderKey::new(page, 1.0, 0), bmp(1000, 700));
        }
        assert!(cache.len() < 6, "old pages are evicted");
        assert!(cache.memory_bytes() <= cache.limit_bytes());
        assert!(
            cache.get(&RenderKey::new(5, 1.0, 0)).is_some(),
            "the page the user is looking at is never evicted"
        );
    }

    #[test]
    fn a_page_bigger_than_the_budget_is_still_kept() {
        let mut cache = PageCache::new(8 * 1024 * 1024);
        // 2000x2000 = 16 MB, twice the budget.
        cache.insert(RenderKey::new(0, 1.0, 0), bmp(2000, 2000));
        assert_eq!(cache.len(), 1, "evicting it would re-render every frame");
        cache.insert(RenderKey::new(1, 1.0, 0), bmp(2000, 2000));
        assert_eq!(cache.len(), 1, "the older page is dropped instead");
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

    #[test]
    fn trim_to_frees_the_least_recently_used_pages_first() {
        let mut cache = PageCache::new(64 * 1024 * 1024);
        // 4 MB per page, ten pages = 40 MB.
        for page in 0..10u32 {
            cache.insert(RenderKey::new(page, 1.0, 0), bmp(1000, 1000));
        }
        assert_eq!(cache.pages(), (0..10).collect::<Vec<u32>>());
        let freed = cache.trim_to(20 * 1024 * 1024, &[]);
        assert!(freed > 0, "something was freed");
        assert!(
            cache.memory_bytes() <= 20 * 1024 * 1024,
            "trimmed to {} bytes",
            cache.memory_bytes()
        );
        // Only as much as needed: the remainder fits one more 4 MB page either way.
        assert!(freed >= 16 * 1024 * 1024 && freed <= 24 * 1024 * 1024, "freed {freed}");
        // The newest page is always kept, even without protection.
        assert!(cache.contains(&RenderKey::new(9, 1.0, 0)));
        assert!(!cache.contains(&RenderKey::new(0, 1.0, 0)));
        assert_eq!(cache.evictions, 5);
    }

    #[test]
    fn trim_to_never_drops_protected_pages() {
        let mut cache = PageCache::new(64 * 1024 * 1024);
        for page in 0..8u32 {
            cache.insert(RenderKey::new(page, 1.0, 0), bmp(1000, 1000));
            // Touch page 2 so it is the most recently used one.
            if page == 2 {
                cache.get(&RenderKey::new(2, 1.0, 0));
            }
        }
        cache.trim_to(8 * 1024 * 1024, &[2]);
        assert!(cache.contains(&RenderKey::new(2, 1.0, 0)), "protected page survived");
        assert_eq!(cache.len(), 2, "the protected page and the newest one remain");
        // A single page is never evicted, whatever the target is.
        cache.trim_to(0, &[]);
        assert_eq!(cache.len(), 1);
    }

    #[test]
    fn keep_only_frees_everything_but_the_listed_pages() {
        let mut cache = PageCache::new(64 * 1024 * 1024);
        for page in 0..5u32 {
            cache.insert(RenderKey::new(page, 1.0, 0), bmp(100, 100));
        }
        let freed = cache.keep_only(&[4]);
        assert_eq!(freed, 4 * 100 * 100 * 4);
        assert_eq!(cache.len(), 1);
        assert_eq!(cache.pages(), vec![4]);
        assert_eq!(cache.newest_page(), Some(4));
        assert_eq!(cache.evictions, 4);
        // Clearing empties the byte count too.
        cache.clear();
        assert_eq!(cache.memory_bytes(), 0);
        assert!(cache.is_empty());
    }
}
