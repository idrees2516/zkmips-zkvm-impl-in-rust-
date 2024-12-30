use std::collections::HashMap;
use super::MemoryError;

#[derive(Debug)]
pub struct PageTable {
    pages: HashMap<u32, Page>,
    free_pages: Vec<u32>,
    page_size: usize,
    next_page_id: u32,
}

#[derive(Debug)]
pub struct Page {
    data: Vec<u8>,
    flags: PageFlags,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PageFlags {
    pub present: bool,
    pub writable: bool,
    pub executable: bool,
    pub user_accessible: bool,
    pub dirty: bool,
    pub accessed: bool,
}

impl PageTable {
    pub fn new(page_size: usize) -> Self {
        Self {
            pages: HashMap::new(),
            free_pages: Vec::new(),
            page_size,
            next_page_id: 0,
        }
    }

    pub fn allocate_page(&mut self) -> Result<u32, MemoryError> {
        let page_id = if let Some(id) = self.free_pages.pop() {
            id
        } else {
            let id = self.next_page_id;
            self.next_page_id = id.checked_add(1).ok_or(MemoryError::OutOfMemory)?;
            id
        };

        let page = Page {
            data: vec![0; self.page_size],
            flags: PageFlags::default(),
        };
        self.pages.insert(page_id, page);

        Ok(page_id)
    }

    pub fn free_page(&mut self, page_id: u32) {
        if self.pages.remove(&page_id).is_some() {
            self.free_pages.push(page_id);
        }
    }

    pub fn read(&self, page_id: u32, offset: usize, size: usize) -> Result<Vec<u8>, MemoryError> {
        let page = self.pages.get(&page_id)
            .ok_or(MemoryError::PageFault(page_id as usize))?;

        if !page.flags.present {
            return Err(MemoryError::PageFault(page_id as usize));
        }

        if offset + size > self.page_size {
            return Err(MemoryError::PageFault(page_id as usize));
        }

        Ok(page.data[offset..offset + size].to_vec())
    }

    pub fn write(&mut self, page_id: u32, offset: usize, data: &[u8]) -> Result<(), MemoryError> {
        let page = self.pages.get_mut(&page_id)
            .ok_or(MemoryError::PageFault(page_id as usize))?;

        if !page.flags.present || !page.flags.writable {
            return Err(MemoryError::PageFault(page_id as usize));
        }

        if offset + data.len() > self.page_size {
            return Err(MemoryError::PageFault(page_id as usize));
        }

        page.data[offset..offset + data.len()].copy_from_slice(data);
        page.flags.dirty = true;
        page.flags.accessed = true;

        Ok(())
    }

    pub fn set_flags(&mut self, page_id: u32, flags: PageFlags) -> Result<(), MemoryError> {
        let page = self.pages.get_mut(&page_id)
            .ok_or(MemoryError::PageFault(page_id as usize))?;
        page.flags = flags;
        Ok(())
    }

    pub fn get_flags(&self, page_id: u32) -> Result<PageFlags, MemoryError> {
        let page = self.pages.get(&page_id)
            .ok_or(MemoryError::PageFault(page_id as usize))?;
        Ok(page.flags)
    }

    pub fn page_size(&self) -> usize {
        self.page_size
    }
}

impl Default for PageFlags {
    fn default() -> Self {
        Self {
            present: true,
            writable: true,
            executable: false,
            user_accessible: true,
            dirty: false,
            accessed: false,
        }
    }
}

impl Page {
    pub fn clear(&mut self) {
        self.data.fill(0);
        self.flags.dirty = false;
        self.flags.accessed = false;
    }
}
