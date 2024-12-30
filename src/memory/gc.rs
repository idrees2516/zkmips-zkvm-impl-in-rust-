use std::collections::HashSet;
use super::{MemorySegment, PageTable};

#[derive(Debug)]
pub struct GarbageCollector {
    threshold: usize,
    marked: HashSet<u32>,
    stats: GCStats,
}

#[derive(Debug, Default)]
pub struct GCStats {
    pub collections: usize,
    pub freed_segments: usize,
    pub freed_pages: usize,
    pub total_time_ms: u64,
}

impl GarbageCollector {
    pub fn new(threshold: usize) -> Self {
        Self {
            threshold,
            marked: HashSet::new(),
            stats: GCStats::default(),
        }
    }

    pub fn collect(&mut self, segments: &mut Vec<MemorySegment>, page_table: &mut PageTable) {
        let start = std::time::Instant::now();

        // Mark phase
        self.mark(segments);

        // Sweep phase
        let (freed_segments, freed_pages) = self.sweep(segments, page_table);

        // Update stats
        self.stats.collections += 1;
        self.stats.freed_segments += freed_segments;
        self.stats.freed_pages += freed_pages;
        self.stats.total_time_ms += start.elapsed().as_millis() as u64;

        // Clear marked set for next collection
        self.marked.clear();
    }

    fn mark(&mut self, segments: &[MemorySegment]) {
        for (i, segment) in segments.iter().enumerate() {
            if self.is_root_segment(segment) {
                self.mark_segment(i as u32);
            }
        }
    }

    fn mark_segment(&mut self, segment_id: u32) {
        if self.marked.insert(segment_id) {
            // Mark any referenced segments
            // This would follow segment references if we had them
        }
    }

    fn sweep(&mut self, segments: &mut Vec<MemorySegment>, page_table: &mut PageTable) -> (usize, usize) {
        let mut freed_segments = 0;
        let mut freed_pages = 0;

        // Remove unmarked segments
        let mut i = 0;
        while i < segments.len() {
            if !self.marked.contains(&(i as u32)) {
                let segment = segments.remove(i);
                freed_segments += 1;
                
                // Free associated pages
                // This assumes segments track their page IDs
                // You would need to add this tracking
                freed_pages += 1;
            } else {
                i += 1;
            }
        }

        (freed_segments, freed_pages)
    }

    fn is_root_segment(&self, segment: &MemorySegment) -> bool {
        // Consider segments as roots if they:
        // 1. Are referenced by active stack frames
        // 2. Are shared segments
        // 3. Have external references
        segment.is_shared() || segment.owner_id().is_some()
    }

    pub fn threshold(&self) -> usize {
        self.threshold
    }

    pub fn stats(&self) -> &GCStats {
        &self.stats
    }
}

impl GCStats {
    pub fn average_collection_time_ms(&self) -> f64 {
        if self.collections == 0 {
            0.0
        } else {
            self.total_time_ms as f64 / self.collections as f64
        }
    }

    pub fn total_freed_memory(&self) -> usize {
        self.freed_segments + self.freed_pages
    }
}
