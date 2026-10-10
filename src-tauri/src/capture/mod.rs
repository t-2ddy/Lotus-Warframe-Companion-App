//! Capture: find the inventory JSON in the game's memory and copy it out (read-only).
//!
//! The game receives its inventory from `inventory.php` as one large UTF-8 JSON document and
//! holds the text only briefly after `OnInventoryResults` appears in EE.log. We search memory
//! for the document's opening bytes, cut out the object, and keep the largest copy that parses.

mod json;
#[cfg(windows)]
mod process;
mod source;

use memchr::memmem::Finder;

pub use json::{object_end, ObjectEnd};
#[cfg(windows)]
pub use process::{find_pid, ProcessSource, Scope, GAME_EXE};
pub use source::{FileSource, MemorySource, Region, SliceSource};

/// The inventory document starts with these bytes (confirmed in Phase 1).
pub const SIGNATURE: &[u8] = br#"{"SubscribedToEmails":"#;
/// Keys a real inventory must contain; rules out other objects that share the opening.
const REQUIRED_KEYS: [&[u8]; 2] = [br#""MiscItems":"#, br#""XPInfo":"#];
/// A real inventory is around 1 MB; anything this small is something else.
const MIN_DOC: usize = 64 * 1024;
/// Stop following a candidate past this size.
const MAX_DOC: usize = 32 * 1024 * 1024;
/// How much memory to read per search step.
const CHUNK: usize = 4 * 1024 * 1024;
/// First read size when following a candidate; doubled until the object closes.
const FIRST_READ: usize = 1024 * 1024;

/// A copy of the inventory JSON taken from memory.
#[derive(Debug, Clone)]
pub struct Snapshot {
    /// Where it was found (an address in the game, or a file offset for dumps).
    pub address: u64,
    pub bytes: Vec<u8>,
}

/// A candidate that started with the signature but wasn't accepted.
#[derive(Debug, Clone)]
pub struct Rejection {
    pub address: u64,
    pub reason: String,
    /// The bytes read for this candidate (up to the point it was rejected), for inspection.
    pub sample: Vec<u8>,
}

/// Rejections kept per scan; enough to diagnose without holding lots of memory.
const MAX_REJECTIONS: usize = 4;

/// What the scan looked at, for diagnostics.
#[derive(Debug, Default, Clone)]
pub struct ScanStats {
    pub regions: usize,
    pub bytes_scanned: u64,
    pub candidates: usize,
    pub rejections: Vec<Rejection>,
}

/// Scans every region and returns the best inventory copy found, if any.
pub fn find_inventory(src: &dyn MemorySource) -> (Option<Snapshot>, ScanStats) {
    let finder = Finder::new(SIGNATURE);
    let overlap = SIGNATURE.len() - 1;
    let mut stats = ScanStats::default();
    let mut best: Option<Snapshot> = None;
    let mut buf = vec![0u8; CHUNK];

    for region in src.regions() {
        stats.regions += 1;
        let mut pos = region.base;
        while pos < region.end() {
            let want = CHUNK.min((region.end() - pos) as usize);
            let got = src.read(pos, &mut buf[..want]);
            if got == 0 {
                // Unreadable page range: skip this chunk and keep going.
                pos += want as u64;
                continue;
            }
            stats.bytes_scanned += got as u64;
            for hit in finder.find_iter(&buf[..got]) {
                let addr = pos + hit as u64;
                stats.candidates += 1;
                match follow_candidate(src, addr) {
                    Ok(bytes) => {
                        if best.as_ref().map_or(true, |b| bytes.len() > b.bytes.len()) {
                            best = Some(Snapshot { address: addr, bytes });
                        }
                    }
                    Err((reason, sample)) => {
                        if stats.rejections.len() < MAX_REJECTIONS {
                            stats.rejections.push(Rejection { address: addr, reason, sample });
                        }
                    }
                }
            }
            if got < want || pos + got as u64 >= region.end() {
                pos += got.max(1) as u64;
            } else {
                // Step back a little so a signature split across two chunks is still found.
                pos += (got - overlap) as u64;
            }
        }
    }
    (best, stats)
}

/// Reads forward from a signature hit until the object closes, then checks that it's an inventory.
/// On rejection returns the reason and the bytes that were read.
///
/// Reading continues past the end of the region the hit was found in: a copy can start near the
/// end of one Windows memory region and carry on into the next (confirmed in Phase 2). It stops
/// only when the object closes, the memory becomes unreadable, or the size cap is reached.
fn follow_candidate(src: &dyn MemorySource, addr: u64) -> Result<Vec<u8>, (String, Vec<u8>)> {
    let mut buf = Vec::new();
    let mut step = FIRST_READ;
    loop {
        let want = step.min(MAX_DOC - buf.len());
        let more = read_more(src, addr, &mut buf, want);
        match object_end(&buf) {
            ObjectEnd::Complete(end) => {
                buf.truncate(end);
                return match check_inventory(&buf) {
                    Ok(()) => Ok(buf),
                    Err(reason) => Err((reason, buf)),
                };
            }
            ObjectEnd::Invalid(at) => {
                let byte = buf.get(at).copied().unwrap_or(0);
                return Err((format!("not clean JSON at byte {at} (0x{byte:02x})"), buf));
            }
            ObjectEnd::Incomplete if buf.len() >= MAX_DOC => {
                return Err((format!("unclosed after {} bytes (size cap)", buf.len()), buf));
            }
            ObjectEnd::Incomplete if !more => {
                return Err((format!("unclosed after {} bytes (memory after it is unreadable)", buf.len()), buf));
            }
            ObjectEnd::Incomplete => step *= 2,
        }
    }
}

/// Size of a memory page; used to find exactly where readable memory stops.
const PAGE: u64 = 4096;

/// Appends up to `want` bytes read from `addr + buf.len()`. Returns false if memory ran out.
///
/// A single large read fails as a whole if any page in it is unreadable, so on a short read
/// this falls back to reading page by page to keep everything up to the first bad page.
fn read_more(src: &dyn MemorySource, addr: u64, buf: &mut Vec<u8>, want: usize) -> bool {
    if want == 0 {
        return false;
    }
    let start = buf.len();
    buf.resize(start + want, 0);
    let got = src.read(addr + start as u64, &mut buf[start..]);
    if got == want {
        return true;
    }
    let mut filled = start + got;
    while filled < start + want {
        let at = addr + filled as u64;
        let page_left = (PAGE - at % PAGE) as usize;
        let n = page_left.min(start + want - filled);
        let read = src.read(at, &mut buf[filled..filled + n]);
        filled += read;
        if read < n {
            break;
        }
    }
    buf.truncate(filled);
    filled == start + want
}

fn check_inventory(doc: &[u8]) -> Result<(), String> {
    if doc.len() < MIN_DOC {
        return Err(format!("too small ({} bytes)", doc.len()));
    }
    for key in REQUIRED_KEYS {
        if memchr::memmem::find(doc, key).is_none() {
            return Err(format!("missing key {}", String::from_utf8_lossy(key)));
        }
    }
    serde_json::from_slice::<serde::de::IgnoredAny>(doc).map_err(|e| format!("JSON parse error: {e}"))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A fake inventory big enough to pass the size check.
    fn fake_inventory(credits: u32) -> Vec<u8> {
        let items: Vec<String> = (0..4000)
            .map(|i| format!(r#"{{"ItemCount":{i},"ItemType":"/Lotus/Types/Items/MiscItems/Test{i}"}}"#))
            .collect();
        format!(
            r#"{{"SubscribedToEmails":0,"RegularCredits":{credits},"MiscItems":[{}],"XPInfo":[]}}"#,
            items.join(",")
        )
        .into_bytes()
    }

    fn surround(parts: &[&[u8]]) -> Vec<u8> {
        let mut mem = vec![0xCDu8; 10_000];
        for p in parts {
            mem.extend_from_slice(p);
            mem.extend(std::iter::repeat(0u8).take(3_000_000));
        }
        mem
    }

    #[test]
    fn finds_inventory_among_garbage() {
        let inv = fake_inventory(42);
        let mem = surround(&[b"junk {\"SubscribedToEmails\":1, not json", &inv]);
        let (snap, stats) = find_inventory(&SliceSource(mem));
        let snap = snap.expect("inventory should be found");
        assert_eq!(snap.bytes, inv);
        assert_eq!(stats.candidates, 2);
    }

    #[test]
    fn rejects_overwritten_copy_and_keeps_good_one() {
        let good = fake_inventory(7);
        let mut broken = fake_inventory(8);
        let mid = broken.len() / 2;
        broken[mid..mid + 16].copy_from_slice(&[0u8; 16]);
        let mem = surround(&[&broken, &good]);
        let (snap, _) = find_inventory(&SliceSource(mem));
        assert_eq!(snap.unwrap().bytes, good);
    }

    #[test]
    fn finds_signature_split_across_chunks() {
        let inv = fake_inventory(1);
        let mut mem = vec![b' '; CHUNK - 5];
        mem.extend_from_slice(&inv);
        mem.extend_from_slice(&[0u8; 100]);
        let (snap, _) = find_inventory(&SliceSource(mem));
        assert_eq!(snap.unwrap().address, (CHUNK - 5) as u64);
    }

    /// Memory split into regions at given offsets, with an optional unreadable range.
    struct SplitSource {
        mem: Vec<u8>,
        splits: Vec<usize>,
        unreadable: Option<std::ops::Range<usize>>,
    }

    impl MemorySource for SplitSource {
        fn regions(&self) -> Vec<Region> {
            let mut edges = vec![0];
            edges.extend(&self.splits);
            edges.push(self.mem.len());
            edges
                .windows(2)
                .map(|w| Region { base: w[0] as u64, size: w[1] - w[0] })
                .filter(|r| self.unreadable.as_ref().map_or(true, |u| !u.contains(&(r.base as usize))))
                .collect()
        }

        fn read(&self, addr: u64, buf: &mut [u8]) -> usize {
            let start = addr as usize;
            let end = (start + buf.len()).min(self.mem.len());
            // Like ReadProcessMemory: a read touching an unreadable page fails as a whole.
            if let Some(u) = &self.unreadable {
                if start < u.end && end > u.start {
                    return 0;
                }
            }
            if start >= end {
                return 0;
            }
            buf[..end - start].copy_from_slice(&self.mem[start..end]);
            end - start
        }
    }

    #[test]
    fn follows_inventory_across_region_boundary() {
        // Phase 2: real copies started near the end of one region and carried on into the next.
        let inv = fake_inventory(3);
        let start = 100_000;
        let mut mem = vec![0u8; start];
        mem.extend_from_slice(&inv);
        mem.extend_from_slice(&[0u8; 50_000]);
        let source = SplitSource { mem, splits: vec![start + 65_088], unreadable: None };
        let (snap, _) = find_inventory(&source);
        assert_eq!(snap.expect("should read across the boundary").bytes, inv);
    }

    #[test]
    fn rejects_copy_cut_off_by_unreadable_memory() {
        let inv = fake_inventory(4);
        let start = 8192;
        let mut mem = vec![0u8; start];
        mem.extend_from_slice(&inv);
        let cut = start + 20 * 4096;
        let len = mem.len();
        let source = SplitSource { mem, splits: vec![cut], unreadable: Some(cut..len) };
        let (snap, stats) = find_inventory(&source);
        assert!(snap.is_none());
        let r = &stats.rejections[0];
        assert!(r.reason.contains("unreadable"), "{}", r.reason);
        assert_eq!(r.sample.len(), cut - start, "keeps everything up to the first bad page");
    }

    #[test]
    fn nothing_found_in_plain_memory() {
        let (snap, _) = find_inventory(&SliceSource(vec![0u8; 5_000_000]));
        assert!(snap.is_none());
    }

    /// Uses the real Phase 1 sample when it exists (it's gitignored, so it may not).
    #[test]
    fn finds_real_fixture() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../fixtures/inventory_sample.json");
        let Ok(inv) = std::fs::read(&path) else {
            eprintln!("skipping: {} not found", path.display());
            return;
        };
        let mem = surround(&[&inv[..inv.len() / 3], &inv]);
        let (snap, _) = find_inventory(&SliceSource(mem));
        assert_eq!(snap.unwrap().bytes, inv);
    }
}
