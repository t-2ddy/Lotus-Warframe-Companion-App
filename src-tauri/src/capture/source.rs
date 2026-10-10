//! Where the scanner reads bytes from: the live game, a dump file, or a test buffer.

use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;
use std::sync::Mutex;

/// A contiguous range of readable memory.
#[derive(Debug, Clone, Copy)]
pub struct Region {
    pub base: u64,
    pub size: usize,
}

impl Region {
    pub fn end(&self) -> u64 {
        self.base + self.size as u64
    }
}

/// Read-only access to some address space.
pub trait MemorySource {
    /// The regions worth scanning.
    fn regions(&self) -> Vec<Region>;
    /// Reads up to `buf.len()` bytes at `addr`. Returns how many bytes were read (0 on failure).
    fn read(&self, addr: u64, buf: &mut [u8]) -> usize;
}

/// An in-memory buffer treated as one region at address 0. Used in tests.
pub struct SliceSource(pub Vec<u8>);

impl MemorySource for SliceSource {
    fn regions(&self) -> Vec<Region> {
        vec![Region { base: 0, size: self.0.len() }]
    }

    fn read(&self, addr: u64, buf: &mut [u8]) -> usize {
        let start = addr as usize;
        if start >= self.0.len() {
            return 0;
        }
        let n = buf.len().min(self.0.len() - start);
        buf[..n].copy_from_slice(&self.0[start..start + n]);
        n
    }
}

/// A file (such as a Task Manager `.dmp`) treated as one region. File offsets stand in for addresses.
pub struct FileSource {
    file: Mutex<File>,
    len: u64,
}

impl FileSource {
    pub fn open(path: &Path) -> std::io::Result<Self> {
        let file = File::open(path)?;
        let len = file.metadata()?.len();
        Ok(Self { file: Mutex::new(file), len })
    }
}

impl MemorySource for FileSource {
    fn regions(&self) -> Vec<Region> {
        vec![Region { base: 0, size: self.len as usize }]
    }

    fn read(&self, addr: u64, buf: &mut [u8]) -> usize {
        let mut file = self.file.lock().unwrap();
        if file.seek(SeekFrom::Start(addr)).is_err() {
            return 0;
        }
        let mut total = 0;
        while total < buf.len() {
            match file.read(&mut buf[total..]) {
                Ok(0) | Err(_) => break,
                Ok(n) => total += n,
            }
        }
        total
    }
}
