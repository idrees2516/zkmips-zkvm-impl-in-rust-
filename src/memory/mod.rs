use std::{
    collections::{HashMap, VecDeque},
    sync::{Arc, RwLock},
};
use blake3::Hash;
use parking_lot::Mutex;
use thiserror::Error;

mod page_table;
mod segment;
mod permissions;
mod gc;

pub use page_table::{PageTable, PageEntry, PageFlags};
pub use segment::{MemorySegment, SegmentType};
pub use permissions::{AccessPermissions, Permission};
pub use gc::{GarbageCollector, GCStats};

#[derive(Error, Debug)]
pub enum MemoryError {
    #[error("Page fault at address {0:#x}")]
    PageFault(usize),
    #[error("Segment fault at address {0:#x}")]
    SegmentFault(usize),
    #[error("Permission denied for address {0:#x}")]
    PermissionDenied(usize),
    #[error("Out of memory")]
    OutOfMemory,
}

pub type MemoryResult<T> = Result<T, MemoryError>;

#[derive(Clone, Debug)]
pub struct MemoryAddress {
    segment_id: u32,
    page_id: u32,
    offset: u32,
}

#[derive(Debug)]
pub struct MemoryManager {
    page_table: Arc<RwLock<PageTable>>,
    segments: Arc<RwLock<Vec<MemorySegment>>>,
    permissions: Arc<RwLock<HashMap<MemoryAddress, AccessPermissions>>>,
    gc: Arc<Mutex<GarbageCollector>>,
    cache: Arc<Mutex<LRUCache>>,
    stats: Arc<RwLock<MemoryStats>>,
}

#[derive(Default, Debug)]
pub struct MemoryStats {
    total_allocations: usize,
    total_deallocations: usize,
    page_faults: usize,
    cache_hits: usize,
    cache_misses: usize,
}

struct LRUCache {
    capacity: usize,
    cache: HashMap<MemoryAddress, Vec<u8>>,
    lru: VecDeque<MemoryAddress>,
}

impl MemoryManager {
    pub fn new(config: MemoryConfig) -> Self {
        Self {
            page_table: Arc::new(RwLock::new(PageTable::new(config.page_size))),
            segments: Arc::new(RwLock::new(Vec::new())),
            permissions: Arc::new(RwLock::new(HashMap::new())),
            gc: Arc::new(Mutex::new(GarbageCollector::new(config.gc_threshold))),
            cache: Arc::new(Mutex::new(LRUCache::new(config.cache_size))),
            stats: Arc::new(RwLock::new(MemoryStats::default())),
        }
    }

    pub fn allocate(&self, size: usize, segment_type: SegmentType) -> MemoryResult<MemoryAddress> {
        let mut segments = self.segments.write();
        let mut page_table = self.page_table.write();

        // Check if GC needed
        if self.should_collect_garbage() {
            self.gc.lock().collect(&mut segments, &mut page_table);
        }

        // Allocate new segment
        let segment = MemorySegment::new(segment_type, size)?;
        let segment_id = segments.len() as u32;
        segments.push(segment);

        // Allocate pages
        let pages_needed = (size + page_table.page_size() - 1) / page_table.page_size();
        let mut page_ids = Vec::with_capacity(pages_needed);

        for _ in 0..pages_needed {
            let page_id = page_table.allocate_page()?;
            page_ids.push(page_id);
        }

        // Set default permissions
        let addr = MemoryAddress {
            segment_id,
            page_id: page_ids[0],
            offset: 0,
        };
        self.permissions.write().insert(addr.clone(), AccessPermissions::default());

        // Update stats
        self.stats.write().total_allocations += 1;

        Ok(addr)
    }

    pub fn deallocate(&self, addr: MemoryAddress) -> MemoryResult<()> {
        let mut segments = self.segments.write();
        let mut page_table = self.page_table.write();
        let mut permissions = self.permissions.write();

        // Remove segment
        segments.remove(addr.segment_id as usize);

        // Free pages
        page_table.free_page(addr.page_id);

        // Remove permissions
        permissions.remove(&addr);

        // Update stats
        self.stats.write().total_deallocations += 1;

        Ok(())
    }

    pub fn read(&self, addr: MemoryAddress, size: usize) -> MemoryResult<Vec<u8>> {
        // Check cache first
        if let Some(data) = self.cache.lock().get(&addr) {
            self.stats.write().cache_hits += 1;
            return Ok(data.clone());
        }
        self.stats.write().cache_misses += 1;

        // Check permissions
        self.check_permissions(&addr, Permission::Read)?;

        // Read from page table
        let page_table = self.page_table.read();
        let data = page_table.read(addr.page_id, addr.offset as usize, size)?;

        // Update cache
        self.cache.lock().insert(addr, data.clone());

        Ok(data)
    }

    pub fn write(&self, addr: MemoryAddress, data: &[u8]) -> MemoryResult<()> {
        // Check permissions
        self.check_permissions(&addr, Permission::Write)?;

        // Write to page table
        let mut page_table = self.page_table.write();
        page_table.write(addr.page_id, addr.offset as usize, data)?;

        // Invalidate cache
        self.cache.lock().remove(&addr);

        Ok(())
    }

    pub fn protect(&self, addr: MemoryAddress, perms: AccessPermissions) -> MemoryResult<()> {
        self.permissions.write().insert(addr, perms);
        Ok(())
    }

    fn should_collect_garbage(&self) -> bool {
        let segments = self.segments.read();
        let total_memory = segments.iter().map(|s| s.size()).sum::<usize>();
        let gc = self.gc.lock();
        total_memory >= gc.threshold()
    }

    fn check_permissions(&self, addr: &MemoryAddress, required: Permission) -> MemoryResult<()> {
        let permissions = self.permissions.read();
        match permissions.get(addr) {
            Some(perms) if perms.has_permission(required) => Ok(()),
            _ => Err(MemoryError::PermissionDenied(addr.segment_id as usize)),
        }
    }
}

impl LRUCache {
    fn new(capacity: usize) -> Self {
        Self {
            capacity,
            cache: HashMap::new(),
            lru: VecDeque::new(),
        }
    }

    fn get(&mut self, key: &MemoryAddress) -> Option<&Vec<u8>> {
        if let Some(pos) = self.lru.iter().position(|x| x == key) {
            self.lru.remove(pos);
            self.lru.push_front(key.clone());
            self.cache.get(key)
        } else {
            None
        }
    }

    fn insert(&mut self, key: MemoryAddress, value: Vec<u8>) {
        if self.cache.len() >= self.capacity {
            if let Some(lru_key) = self.lru.pop_back() {
                self.cache.remove(&lru_key);
            }
        }
        self.cache.insert(key.clone(), value);
        self.lru.push_front(key);
    }

    fn remove(&mut self, key: &MemoryAddress) {
        self.cache.remove(key);
        if let Some(pos) = self.lru.iter().position(|x| x == key) {
            self.lru.remove(pos);
        }
    }
}

#[derive(Clone, Debug)]
pub struct MemoryConfig {
    page_size: usize,
    gc_threshold: usize,
    cache_size: usize,
}
