//! Who holds our heap: one allocation in every `VICTORY_HEAP_SAMPLE` KiB
//! made (and every block of that size or more) is noted with the stack that
//! made it, and forgotten when it is released; the memory line then names
//! the stacks that hold the most of what is still alive. Off unless the
//! variable is set (diagnostics builds only).
//!
//! Our own heap grows by about 150 MB a minute in play (1.0 GB at the
//! start, 3.3 GB after 15 minutes, run play of 2026-11-27), in blocks under
//! 4 KiB, and the memory line's sums say nothing of whose they are.
//!
//! Nothing here allocates while noting: the tables are fixed arrays, the
//! stack is taken with `RtlCaptureStackBackTrace`, and a thread that is
//! already noting (or naming, which does allocate) is not sampled again.

use std::cell::Cell;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicI64, AtomicU64, AtomicUsize, Ordering::Relaxed};

/// Return addresses kept per stack.
const DEPTH: usize = 20;
/// Places for sampled blocks alive (a power of two).
const BLOCKS: usize = 1 << 20;
/// Places for distinct stacks (a power of two).
const STACKS: usize = 1 << 15;

/// Bytes between samples, 0: off.
fn rate() -> u64 {
    static RATE: OnceLock<u64> = OnceLock::new();
    *RATE.get_or_init(|| {
        std::env::var("VICTORY_HEAP_SAMPLE").ok().and_then(|v| v.trim().parse::<u64>().ok()).unwrap_or(0) << 10
    })
}

/// Whether sampling is on, without touching the variable from inside the
/// allocator before it was read once outside it.
static ON: AtomicU64 = AtomicU64::new(0);

/// Reads the switch (from the memory line's thread, not the allocator).
pub(super) fn start() {
    ON.store(rate(), Relaxed);
}

struct Block {
    at: AtomicUsize,
    stack: AtomicUsize,
    weight: AtomicU64,
}

struct Stack {
    hash: AtomicU64,
    frames: [AtomicUsize; DEPTH],
    count: AtomicI64,
    bytes: AtomicI64,
}

static BLOCK_TABLE: [Block; BLOCKS] =
    [const { Block { at: AtomicUsize::new(0), stack: AtomicUsize::new(0), weight: AtomicU64::new(0) } }; BLOCKS];
static STACK_TABLE: [Stack; STACKS] = [const {
    Stack {
        hash: AtomicU64::new(0),
        frames: [const { AtomicUsize::new(0) }; DEPTH],
        count: AtomicI64::new(0),
        bytes: AtomicI64::new(0),
    }
}; STACKS];

thread_local! {
    /// Bytes this thread may still allocate before its next sample.
    static LEFT: Cell<u64> = const { Cell::new(0) };
    /// This thread is noting or naming: its allocations are not sampled.
    static BUSY: Cell<bool> = const { Cell::new(false) };
}

fn spread(x: u64) -> u64 {
    let x = (x ^ (x >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    let x = (x ^ (x >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    x ^ (x >> 31)
}

/// A block of `size` bytes was made at `at`.
pub(super) fn made(at: *mut u8, size: usize) {
    let rate = ON.load(Relaxed);
    if rate == 0 {
        return;
    }
    let size = size as u64;
    let weight = if size >= rate {
        size
    } else {
        let sample = LEFT
            .try_with(|left| {
                let l = left.get();
                if l > size {
                    left.set(l - size);
                    false
                } else {
                    left.set(rate);
                    true
                }
            })
            .unwrap_or(false);
        if !sample {
            return;
        }
        rate
    };
    if BUSY.try_with(|b| b.replace(true)).unwrap_or(true) {
        return;
    }
    note(at as usize, weight);
    let _ = BUSY.try_with(|b| b.set(false));
}

fn note(at: usize, weight: u64) {
    use windows_sys::Win32::System::Diagnostics::Debug::RtlCaptureStackBackTrace;
    let mut frames = [0usize; DEPTH];
    // SAFETY: room for `DEPTH` return addresses.
    let n = unsafe { RtlCaptureStackBackTrace(3, DEPTH as u32, frames.as_mut_ptr().cast(), std::ptr::null_mut()) } as usize;
    let hash = frames[..n].iter().fold(0x9e37_79b9u64, |h, &f| spread(h ^ f as u64)) | 1;
    let mut s = hash as usize & (STACKS - 1);
    let stack = loop {
        let entry = &STACK_TABLE[s];
        match entry.hash.compare_exchange(0, hash, Relaxed, Relaxed) {
            Ok(_) => {
                for (place, &f) in entry.frames.iter().zip(&frames) {
                    place.store(f, Relaxed);
                }
                break s;
            }
            Err(h) if h == hash => break s,
            Err(_) => {}
        }
        s = (s + 1) & (STACKS - 1);
        if s == hash as usize & (STACKS - 1) {
            return; // full
        }
    };
    let _held = EDIT.lock().unwrap_or_else(|e| e.into_inner());
    let mut b = home(at);
    for _ in 0..BLOCKS {
        let place = &BLOCK_TABLE[b];
        if place.at.load(Relaxed) == 0 {
            place.stack.store(stack, Relaxed);
            place.weight.store(weight, Relaxed);
            place.at.store(at, Relaxed);
            STACK_TABLE[stack].count.fetch_add(1, Relaxed);
            STACK_TABLE[stack].bytes.fetch_add(weight as i64, Relaxed);
            return;
        }
        b = (b + 1) & (BLOCKS - 1);
    }
}

/// Where a block's place is first looked for.
fn home(at: usize) -> usize {
    spread(at as u64) as usize & (BLOCKS - 1)
}

/// Taken to add or take out a block (a release of a block never sampled
/// looks without it: at a hundredth full, its home is nearly always empty).
static EDIT: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// The block at `at` was released.
pub(super) fn gone(at: *mut u8) {
    if ON.load(Relaxed) == 0 {
        return;
    }
    let at = at as usize;
    let mut b = home(at);
    loop {
        let now = BLOCK_TABLE[b].at.load(Relaxed);
        if now == 0 {
            return;
        }
        if now == at {
            break;
        }
        b = (b + 1) & (BLOCKS - 1);
    }
    let _held = EDIT.lock().unwrap_or_else(|e| e.into_inner());
    if BLOCK_TABLE[b].at.load(Relaxed) != at {
        return; // moved meanwhile: counted as still alive
    }
    let place = &BLOCK_TABLE[b];
    let stack = place.stack.load(Relaxed);
    STACK_TABLE[stack].count.fetch_sub(1, Relaxed);
    STACK_TABLE[stack].bytes.fetch_sub(place.weight.load(Relaxed) as i64, Relaxed);
    // Linear probing's deletion: later blocks of the run that may stand
    // here move back, so that no lookup stops short of them.
    let mut hole = b;
    let mut j = b;
    loop {
        j = (j + 1) & (BLOCKS - 1);
        let next = BLOCK_TABLE[j].at.load(Relaxed);
        if next == 0 {
            break;
        }
        let k = home(next);
        let between = if hole <= j { hole < k && k <= j } else { hole < k || k <= j };
        if !between {
            let (from, to) = (&BLOCK_TABLE[j], &BLOCK_TABLE[hole]);
            to.stack.store(from.stack.load(Relaxed), Relaxed);
            to.weight.store(from.weight.load(Relaxed), Relaxed);
            to.at.store(next, Relaxed);
            hole = j;
        }
    }
    BLOCK_TABLE[hole].at.store(0, Relaxed);
}

/// The stacks holding the most sampled bytes alive, named.
pub(super) fn report() {
    if ON.load(Relaxed) == 0 {
        return;
    }
    // A line a minute is enough.
    static LAST: std::sync::Mutex<Option<std::time::Instant>> = std::sync::Mutex::new(None);
    {
        let mut last = LAST.lock().unwrap_or_else(|e| e.into_inner());
        if last.is_some_and(|t| t.elapsed().as_secs() < 60) {
            return;
        }
        *last = Some(std::time::Instant::now());
    }
    let _ = BUSY.try_with(|b| b.set(true));
    struct Named(crate::timeline::report::Names);
    // SAFETY: the process handle inside is the pseudo handle, good on any
    // thread; the namer is used under the lock alone.
    unsafe impl Send for Named {}
    static NAMES: std::sync::Mutex<Option<Named>> = std::sync::Mutex::new(None);
    let mut names = NAMES.lock().unwrap_or_else(|e| e.into_inner());
    let names = &mut names.get_or_insert_with(|| Named(crate::timeline::report::Names::new())).0;
    let mut top: Vec<(i64, i64, usize)> = STACK_TABLE
        .iter()
        .enumerate()
        .filter(|(_, s)| s.hash.load(Relaxed) != 0)
        .map(|(i, s)| (s.bytes.load(Relaxed), s.count.load(Relaxed), i))
        .filter(|t| t.0 > 0)
        .collect();
    let total: i64 = top.iter().map(|t| t.0).sum();
    // Sites by the code that asked (several stacks can share one).
    let mut sites: std::collections::HashMap<String, (i64, i64)> = std::collections::HashMap::new();
    top.sort_unstable_by_key(|t| std::cmp::Reverse(t.0));
    for &(bytes, count, i) in &top {
        let mut parts: Vec<String> = Vec::new();
        for f in STACK_TABLE[i].frames.iter().map(|f| f.load(Relaxed)).take_while(|&f| f != 0) {
            let name = names.of(f);
            let plain = name.trim_start_matches('<').trim_start_matches("impl$").trim_start_matches(|c: char| c.is_ascii_digit());
            let skipped = ["std::", "core::", "alloc::", "hashbrown::", "windows_core::", "windows::"]
                .iter()
                .any(|p| plain.starts_with(p))
                || name.contains("copystat::")
                || ["KERNEL", "ntdll", "kernel"].iter().any(|p| name.starts_with(p));
            if !skipped {
                parts.push(name.replace("victory_gpu::", "").replace("victory_hle::", "hle::"));
            }
            if parts.len() == 4 {
                break;
            }
        }
        let e = sites.entry(parts.join(" < ")).or_default();
        e.0 += bytes;
        e.1 += count;
    }
    let mut sites: Vec<(String, (i64, i64))> = sites.into_iter().collect();
    sites.sort_unstable_by_key(|s| std::cmp::Reverse(s.1.0));
    let text: Vec<String> = sites
        .iter()
        .take(14)
        .map(|(site, (bytes, count))| format!("{:.0} MB ({count} samples) {site}", *bytes as f64 / (1 << 20) as f64))
        .collect();
    tracing::info!(
        "heap samples (VICTORY_HEAP_SAMPLE): {:.0} MB of our heap alive by sample, the sites that hold most: {}",
        total as f64 / (1 << 20) as f64,
        text.join("; ")
    );
    let _ = BUSY.try_with(|b| b.set(false));
}
