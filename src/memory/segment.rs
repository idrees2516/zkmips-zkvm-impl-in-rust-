use super::MemoryError;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SegmentType {
    Code,
    Data,
    Stack,
    Heap,
}

#[derive(Debug)]
pub struct MemorySegment {
    segment_type: SegmentType,
    base: usize,
    size: usize,
    max_size: usize,
    is_growable: bool,
    metadata: SegmentMetadata,
}

#[derive(Debug)]
struct SegmentMetadata {
    creation_time: std::time::SystemTime,
    last_access: std::time::SystemTime,
    reference_count: usize,
    is_shared: bool,
    owner_id: Option<u32>,
}

impl MemorySegment {
    pub fn new(segment_type: SegmentType, size: usize) -> Result<Self, MemoryError> {
        if size == 0 {
            return Err(MemoryError::OutOfMemory);
        }

        let (max_size, is_growable) = match segment_type {
            SegmentType::Code => (size, false),
            SegmentType::Data => (size * 2, true),
            SegmentType::Stack => (1024 * 1024, true), // 1MB stack
            SegmentType::Heap => (usize::MAX, true),
        };

        Ok(Self {
            segment_type,
            base: 0, // Will be set during allocation
            size,
            max_size,
            is_growable,
            metadata: SegmentMetadata::new(),
        })
    }

    pub fn resize(&mut self, new_size: usize) -> Result<(), MemoryError> {
        if !self.is_growable {
            return Err(MemoryError::SegmentFault(self.base));
        }

        if new_size > self.max_size {
            return Err(MemoryError::OutOfMemory);
        }

        self.size = new_size;
        Ok(())
    }

    pub fn contains(&self, address: usize) -> bool {
        address >= self.base && address < self.base + self.size
    }

    pub fn offset(&self, address: usize) -> Option<usize> {
        if self.contains(address) {
            Some(address - self.base)
        } else {
            None
        }
    }

    pub fn segment_type(&self) -> SegmentType {
        self.segment_type
    }

    pub fn size(&self) -> usize {
        self.size
    }

    pub fn base(&self) -> usize {
        self.base
    }

    pub fn set_base(&mut self, base: usize) {
        self.base = base;
    }

    pub fn is_growable(&self) -> bool {
        self.is_growable
    }

    pub fn max_size(&self) -> usize {
        self.max_size
    }

    pub fn increment_ref_count(&mut self) {
        self.metadata.reference_count += 1;
    }

    pub fn decrement_ref_count(&mut self) -> usize {
        self.metadata.reference_count = self.metadata.reference_count.saturating_sub(1);
        self.metadata.reference_count
    }

    pub fn set_shared(&mut self, shared: bool) {
        self.metadata.is_shared = shared;
    }

    pub fn is_shared(&self) -> bool {
        self.metadata.is_shared
    }

    pub fn set_owner(&mut self, owner_id: u32) {
        self.metadata.owner_id = Some(owner_id);
    }

    pub fn owner_id(&self) -> Option<u32> {
        self.metadata.owner_id
    }

    pub fn update_access_time(&mut self) {
        self.metadata.last_access = std::time::SystemTime::now();
    }

    pub fn last_access_time(&self) -> std::time::SystemTime {
        self.metadata.last_access
    }

    pub fn creation_time(&self) -> std::time::SystemTime {
        self.metadata.creation_time
    }
}

impl SegmentMetadata {
    fn new() -> Self {
        Self {
            creation_time: std::time::SystemTime::now(),
            last_access: std::time::SystemTime::now(),
            reference_count: 1,
            is_shared: false,
            owner_id: None,
        }
    }
}
