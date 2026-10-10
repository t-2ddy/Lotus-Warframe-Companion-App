//! Read-only access to a running process's memory (Windows).
//!
//! The handle is opened with query + read rights only: no write, no thread or injection rights.

use std::ffi::c_void;
use std::mem::size_of;

use windows::Win32::Foundation::{CloseHandle, HANDLE};
use windows::Win32::System::Diagnostics::Debug::ReadProcessMemory;
use windows::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W, TH32CS_SNAPPROCESS,
};
use windows::Win32::System::Memory::{
    VirtualQueryEx, MEMORY_BASIC_INFORMATION, MEM_COMMIT, MEM_IMAGE, MEM_MAPPED, MEM_PRIVATE,
    PAGE_EXECUTE_READ, PAGE_EXECUTE_READWRITE, PAGE_EXECUTE_WRITECOPY, PAGE_GUARD, PAGE_NOACCESS,
    PAGE_PROTECTION_FLAGS, PAGE_READONLY, PAGE_READWRITE, PAGE_WRITECOPY,
};
use windows::Win32::System::Threading::{OpenProcess, PROCESS_QUERY_INFORMATION, PROCESS_VM_READ};

use super::source::{MemorySource, Region};

pub const GAME_EXE: &str = "Warframe.x64.exe";

/// Returns the process id of the first running process with this exe name, if any.
pub fn find_pid(exe_name: &str) -> Option<u32> {
    unsafe {
        let snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0).ok()?;
        let mut entry = PROCESSENTRY32W {
            dwSize: size_of::<PROCESSENTRY32W>() as u32,
            ..Default::default()
        };
        let mut found = None;
        if Process32FirstW(snapshot, &mut entry).is_ok() {
            loop {
                let len = entry.szExeFile.iter().position(|&c| c == 0).unwrap_or(entry.szExeFile.len());
                let name = String::from_utf16_lossy(&entry.szExeFile[..len]);
                if name.eq_ignore_ascii_case(exe_name) {
                    found = Some(entry.th32ProcessID);
                    break;
                }
                if Process32NextW(snapshot, &mut entry).is_err() {
                    break;
                }
            }
        }
        let _ = CloseHandle(snapshot);
        found
    }
}

/// Which memory regions a scan covers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scope {
    /// Committed, private, plain read/write pages: ordinary heap memory.
    Heap,
    /// Readable private and mapped memory, skipping loaded program files (image memory).
    Data,
    /// Every committed region we're allowed to read, including mapped and image memory.
    AllReadable,
}

/// An open, read-only handle to another process.
pub struct ProcessSource {
    handle: HANDLE,
    scope: Scope,
}

// The handle is only used for read calls, which are safe from any thread.
unsafe impl Send for ProcessSource {}
unsafe impl Sync for ProcessSource {}

impl ProcessSource {
    pub fn open(pid: u32, scope: Scope) -> windows::core::Result<Self> {
        let handle = unsafe { OpenProcess(PROCESS_QUERY_INFORMATION | PROCESS_VM_READ, false, pid)? };
        Ok(Self { handle, scope })
    }

    fn query(&self, addr: usize) -> Option<MEMORY_BASIC_INFORMATION> {
        let mut info = MEMORY_BASIC_INFORMATION::default();
        let written = unsafe {
            VirtualQueryEx(
                self.handle,
                Some(addr as *const c_void),
                &mut info,
                size_of::<MEMORY_BASIC_INFORMATION>(),
            )
        };
        (written != 0 && info.RegionSize != 0).then_some(info)
    }

    /// Describes the region holding `addr`, such as "private RW", for diagnostics.
    pub fn describe(&self, addr: u64) -> String {
        let Some(info) = self.query(addr as usize) else {
            return "unknown".into();
        };
        let kind = match info.Type {
            t if t == MEM_PRIVATE => "private",
            t if t == MEM_MAPPED => "mapped",
            t if t == MEM_IMAGE => "image",
            _ => "other",
        };
        format!("{kind} {}", protection_name(info.Protect))
    }

    fn wanted(&self, info: &MEMORY_BASIC_INFORMATION) -> bool {
        if info.State != MEM_COMMIT {
            return false;
        }
        match self.scope {
            Scope::Heap => info.Type == MEM_PRIVATE && info.Protect == PAGE_READWRITE,
            Scope::Data => info.Type != MEM_IMAGE && is_readable(info.Protect),
            Scope::AllReadable => is_readable(info.Protect),
        }
    }
}

fn is_readable(p: PAGE_PROTECTION_FLAGS) -> bool {
    if (p.0 & PAGE_GUARD.0) != 0 || (p.0 & PAGE_NOACCESS.0) != 0 {
        return false;
    }
    let base = p.0 & 0xFF;
    [
        PAGE_READONLY,
        PAGE_READWRITE,
        PAGE_WRITECOPY,
        PAGE_EXECUTE_READ,
        PAGE_EXECUTE_READWRITE,
        PAGE_EXECUTE_WRITECOPY,
    ]
    .iter()
    .any(|f| f.0 == base)
}

fn protection_name(p: PAGE_PROTECTION_FLAGS) -> &'static str {
    match PAGE_PROTECTION_FLAGS(p.0 & 0xFF) {
        f if f == PAGE_READONLY => "R",
        f if f == PAGE_READWRITE => "RW",
        f if f == PAGE_WRITECOPY => "WC",
        f if f == PAGE_EXECUTE_READ => "RX",
        f if f == PAGE_EXECUTE_READWRITE => "RWX",
        f if f == PAGE_EXECUTE_WRITECOPY => "XWC",
        _ => "?",
    }
}

impl Drop for ProcessSource {
    fn drop(&mut self) {
        unsafe {
            let _ = CloseHandle(self.handle);
        }
    }
}

impl MemorySource for ProcessSource {
    /// The regions selected by this source's [`Scope`].
    fn regions(&self) -> Vec<Region> {
        let mut regions = Vec::new();
        let mut addr: usize = 0;
        while let Some(info) = self.query(addr) {
            if self.wanted(&info) {
                regions.push(Region { base: info.BaseAddress as u64, size: info.RegionSize });
            }
            match (info.BaseAddress as usize).checked_add(info.RegionSize) {
                Some(next) if next > addr => addr = next,
                _ => break,
            }
        }
        regions
    }

    fn read(&self, addr: u64, buf: &mut [u8]) -> usize {
        let mut read = 0usize;
        let ok = unsafe {
            ReadProcessMemory(
                self.handle,
                addr as *const c_void,
                buf.as_mut_ptr() as *mut c_void,
                buf.len(),
                Some(&mut read),
            )
        };
        if ok.is_ok() { read } else { read.min(buf.len()) }
    }
}
