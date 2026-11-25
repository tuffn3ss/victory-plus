//! Staging memory for texture uploads, cut from a few large upload heaps
//! of our own instead of one committed upload buffer per texture.
//!
//! Every texture the title streams in needed a staging buffer. Past the
//! 256 MB pool of spare ones (`texture.rs`), each was a new committed
//! upload buffer, made and freed again: 23,877 (16.4 GB) in a five-minute
//! run on an RX 7800 XT, about a quarter of a millisecond each to make.
//! With this, 2,701 (312 MB) and 16 heaps of 64 MiB. Measured at the
//! intro's first rooms (about 4,800 draws a frame) it moved neither the
//! frame time (167.4 against 166.8 ms) nor memory: the 14 GB of
//! write-combined memory committed there is not these buffers (0.85 GB of
//! upload buffers were alive, counted by a destruction callback on each).
//! What it saves is the making and freeing in streaming bursts.
//!
//! Here a staging buffer is a placed buffer in a slot of a 64 MiB upload
//! heap, the slot a power of two from 64 KiB to 32 MiB (one size to a
//! heap). The slot goes back when the placed buffer is destroyed: a small
//! COM object is attached to it as private data, and D3D12 releases that
//! object when the buffer goes. So a slot is used again exactly when a
//! freed buffer's memory could have been, never while a list still holds
//! the buffer. A heap with no slot in use is kept while the empty heaps
//! come to [`KEEP`] bytes or less, else let go.
//!
//! `VICTORY_STAGING_ARENA=0`: committed buffers, as before.

use std::sync::Mutex;
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};

use windows::Win32::Graphics::Direct3D12::*;
use windows::Win32::Graphics::Dxgi::Common::DXGI_SAMPLE_DESC;
use windows::core::{GUID, HRESULT, IUnknown, Interface};

/// One heap's bytes.
const HEAP: u64 = 64 << 20;
/// The smallest and largest slot.
const SMALLEST: u64 = 64 << 10;
const LARGEST: u64 = 32 << 20;
/// Size classes: 64 KiB, 128 KiB, ... 32 MiB.
const CLASSES: usize = (LARGEST.trailing_zeros() - SMALLEST.trailing_zeros() + 1) as usize;
/// The most bytes of heaps with no slot in use that are kept.
const KEEP: u64 = 256 << 20;

/// Whether staging buffers come from here.
pub(crate) fn on() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| std::env::var_os("VICTORY_STAGING_ARENA").is_none_or(|v| v != "0"))
}

struct Heap {
    heap: ID3D12Heap,
    /// Slots not in use, and whether each was ever used (then it is zeroed
    /// before it is given out again).
    free: Vec<(u32, bool)>,
    used: u32,
    /// Distinguishes this heap from a later one in the same place of the
    /// list (slots name their heap by it).
    id: u64,
}

// SAFETY: a D3D12 heap, which any thread may use.
unsafe impl Send for Heap {}

#[derive(Default)]
struct Arena {
    classes: [Vec<Heap>; CLASSES],
    next_id: u64,
    /// Bytes of heaps with no slot in use.
    empty: u64,
}

static ARENA: Mutex<Arena> = Mutex::new(Arena {
    classes: [const { Vec::new() }; CLASSES],
    next_id: 1,
    empty: 0,
});

/// Heaps alive, and slots in use with their bytes (for the memory line).
pub(crate) static STATS: [AtomicU64; 3] = [const { AtomicU64::new(0) }; 3];

fn class_of(bytes: u64) -> Option<usize> {
    let size = bytes.max(SMALLEST).next_power_of_two();
    (size <= LARGEST).then(|| (size.trailing_zeros() - SMALLEST.trailing_zeros()) as usize)
}

fn class_size(class: usize) -> u64 {
    SMALLEST << class
}

/// A mapped staging buffer of at least `total` bytes, all zeros, from the
/// arena; `None` when it is too big for a slot or making it failed (the
/// caller then makes a committed buffer).
pub(crate) fn staging(device: &ID3D12Device, total: u64) -> Option<(ID3D12Resource, *mut u8)> {
    let class = class_of(total)?;
    let size = class_size(class);
    let (heap, id, slot, dirty) = take(device, class)?;
    let desc = D3D12_RESOURCE_DESC {
        Dimension: D3D12_RESOURCE_DIMENSION_BUFFER,
        Width: size,
        Height: 1,
        DepthOrArraySize: 1,
        MipLevels: 1,
        SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
        Layout: D3D12_TEXTURE_LAYOUT_ROW_MAJOR,
        ..Default::default()
    };
    let mut buffer: Option<ID3D12Resource> = None;
    let mut data = std::ptr::null_mut();
    // SAFETY: a placed buffer in a live upload heap, in a slot nothing
    // else uses; mapped whole.
    let made = unsafe {
        device
            .CreatePlacedResource(&heap, u64::from(slot) * size, &desc, D3D12_RESOURCE_STATE_GENERIC_READ, None, &mut buffer)
            .and_then(|()| {
                let b = buffer.as_ref().ok_or_else(windows::core::Error::empty)?;
                b.Map(0, None, Some(&mut data))
            })
    };
    let buffer = match (made, buffer) {
        (Ok(()), Some(buffer)) => buffer,
        (result, _) => {
            if let Err(e) = result {
                tracing::warn!(error = %e, size, "no placed staging buffer");
            }
            give_back(class, id, slot, false);
            return None;
        }
    };
    if !watch(&buffer, class, id, slot) {
        // The slot stays taken for the run (it cannot be known when this
        // buffer goes); the buffer is used as it is.
        tracing::warn!(size, "staging slot without a destruction callback");
    }
    if dirty {
        // SAFETY: the buffer's own mapping, `size` bytes.
        unsafe { std::ptr::write_bytes(data.cast::<u8>(), 0, total.min(size) as usize) };
    }
    STATS[1].fetch_add(1, Ordering::Relaxed);
    STATS[2].fetch_add(size, Ordering::Relaxed);
    Some((buffer, data.cast()))
}

/// A free slot of `class`: its heap, the heap's id, the slot and whether
/// it was used before. Makes a heap when none has a free slot.
fn take(device: &ID3D12Device, class: usize) -> Option<(ID3D12Heap, u64, u32, bool)> {
    {
        let mut arena = ARENA.lock().unwrap_or_else(|e| e.into_inner());
        let mut emptied = 0;
        let found = arena.classes[class].iter_mut().find(|h| !h.free.is_empty()).map(|h| {
            let (slot, dirty) = h.free.pop().expect("a free slot");
            if h.used == 0 {
                emptied = HEAP;
            }
            h.used += 1;
            (h.heap.clone(), h.id, slot, dirty)
        });
        arena.empty -= emptied;
        if found.is_some() {
            return found;
        }
    }
    let desc = D3D12_HEAP_DESC {
        SizeInBytes: HEAP,
        Properties: D3D12_HEAP_PROPERTIES { Type: D3D12_HEAP_TYPE_UPLOAD, ..Default::default() },
        Alignment: 0,
        Flags: D3D12_HEAP_FLAG_ALLOW_ONLY_BUFFERS,
    };
    let mut heap: Option<ID3D12Heap> = None;
    // SAFETY: plain creation of an upload heap.
    if let Err(e) = unsafe { device.CreateHeap(&desc, &mut heap) } {
        tracing::warn!(error = %e, "no staging heap");
        return None;
    }
    let heap = heap?;
    let slots = (HEAP / class_size(class)) as u32;
    let mut arena = ARENA.lock().unwrap_or_else(|e| e.into_inner());
    let id = arena.next_id;
    arena.next_id += 1;
    // Slot 0 is given out now; the others are free, never used.
    arena.classes[class].push(Heap {
        heap: heap.clone(),
        free: (1..slots).rev().map(|s| (s, false)).collect(),
        used: 1,
        id,
    });
    let alive = STATS[0].fetch_add(1, Ordering::Relaxed) + 1;
    if alive.is_power_of_two() || alive % 16 == 0 {
        tracing::info!(
            heaps = alive,
            mb = alive * (HEAP >> 20),
            slots_in_use = STATS[1].load(Ordering::Relaxed),
            "staging arena grew"
        );
    }
    Some((heap, id, 0, false))
}

/// A slot is free again (its buffer was destroyed, or never made).
fn give_back(class: usize, id: u64, slot: u32, was_used: bool) {
    let mut gone = None;
    {
        let mut arena = ARENA.lock().unwrap_or_else(|e| e.into_inner());
        let arena = &mut *arena;
        let heaps = &mut arena.classes[class];
        let Some(at) = heaps.iter().position(|h| h.id == id) else {
            return;
        };
        let h = &mut heaps[at];
        h.free.push((slot, was_used));
        h.used -= 1;
        if h.used == 0 {
            if arena.empty + HEAP > KEEP {
                gone = Some(heaps.swap_remove(at));
            } else {
                arena.empty += HEAP;
            }
        }
    }
    // Let go outside the lock (releasing a heap may call into the driver).
    if gone.take().is_some() {
        STATS[0].fetch_sub(1, Ordering::Relaxed);
    }
}

/// The private-data object whose release gives the slot back.
#[repr(C)]
struct Watch {
    vtbl: *const WatchVtbl,
    refs: AtomicU32,
    class: usize,
    id: u64,
    slot: u32,
    size: u64,
}

#[repr(C)]
struct WatchVtbl {
    query: unsafe extern "system" fn(*mut Watch, *const GUID, *mut *mut std::ffi::c_void) -> HRESULT,
    add_ref: unsafe extern "system" fn(*mut Watch) -> u32,
    release: unsafe extern "system" fn(*mut Watch) -> u32,
}

static VTBL: WatchVtbl = WatchVtbl {
    query: watch_query,
    add_ref: watch_add_ref,
    release: watch_release,
};

/// The private data's name on each staging buffer.
const SLOT_GUID: GUID = GUID::from_u128(0x7a3c_51e0_9d2b_4c6e_8f01_5a7a_9e11_0001);

unsafe extern "system" fn watch_query(this: *mut Watch, iid: *const GUID, out: *mut *mut std::ffi::c_void) -> HRESULT {
    // SAFETY: the caller passes valid pointers; `this` is live.
    unsafe {
        if *iid == IUnknown::IID {
            watch_add_ref(this);
            *out = this.cast();
            HRESULT(0)
        } else {
            *out = std::ptr::null_mut();
            windows::Win32::Foundation::E_NOINTERFACE
        }
    }
}

unsafe extern "system" fn watch_add_ref(this: *mut Watch) -> u32 {
    // SAFETY: a live watch.
    unsafe { (*this).refs.fetch_add(1, Ordering::AcqRel) + 1 }
}

unsafe extern "system" fn watch_release(this: *mut Watch) -> u32 {
    // SAFETY: a live watch; the last reference frees it.
    unsafe {
        let left = (*this).refs.fetch_sub(1, Ordering::AcqRel) - 1;
        if left == 0 {
            let w = Box::from_raw(this);
            STATS[1].fetch_sub(1, Ordering::Relaxed);
            STATS[2].fetch_sub(w.size, Ordering::Relaxed);
            give_back(w.class, w.id, w.slot, true);
        }
        left
    }
}

/// Attaches the slot's watch to `buffer`; false if D3D12 refused it.
fn watch(buffer: &ID3D12Resource, class: usize, id: u64, slot: u32) -> bool {
    let w = Box::into_raw(Box::new(Watch {
        vtbl: &VTBL,
        refs: AtomicU32::new(1),
        class,
        id,
        slot,
        size: class_size(class),
    }));
    // SAFETY: `w` is a COM object with one reference, which `unknown` takes.
    let unknown = unsafe { IUnknown::from_raw(w.cast()) };
    // SAFETY: a live buffer; D3D12 takes a reference of its own and
    // releases it when the buffer is destroyed.
    let set = unsafe { buffer.SetPrivateDataInterface(&SLOT_GUID, &unknown) }.is_ok();
    if !set {
        // Never released: the slot is not given back while the buffer may
        // still be in use.
        std::mem::forget(unknown);
    }
    set
}

#[cfg(test)]
mod tests {
    use super::*;

    /// On the PC's card: slots come back when their buffers are destroyed,
    /// and a slot used again reads as zeros.
    #[test]
    #[ignore]
    fn card_slots_come_back_and_are_zeroed() {
        // SAFETY: plain device creation.
        let device = unsafe {
            let mut device: Option<ID3D12Device> = None;
            D3D12CreateDevice(None, windows::Win32::Graphics::Direct3D::D3D_FEATURE_LEVEL_12_0, &mut device).unwrap();
            device.unwrap()
        };
        let before = STATS[1].load(Ordering::Relaxed);
        let mut held = Vec::new();
        for _ in 0..300 {
            let (b, data) = staging(&device, 200 << 10).expect("a slot");
            // SAFETY: the slot's mapping, 256 KiB.
            unsafe { std::ptr::write_bytes(data, 0xAB, 200 << 10) };
            held.push(b);
        }
        assert_eq!(STATS[1].load(Ordering::Relaxed), before + 300);
        let heaps = STATS[0].load(Ordering::Relaxed);
        drop(held);
        assert_eq!(STATS[1].load(Ordering::Relaxed), before);
        let (_b, data) = staging(&device, 200 << 10).expect("a slot");
        // SAFETY: as above.
        let bytes = unsafe { std::slice::from_raw_parts(data, 200 << 10) };
        assert!(bytes.iter().all(|&b| b == 0), "a used slot is zeroed");
        assert!(STATS[0].load(Ordering::Relaxed) <= heaps, "no new heap for a slot that came back");
    }

    #[test]
    fn classes_are_powers_of_two_from_64_kib_to_32_mib() {
        assert_eq!(class_of(1), Some(0));
        assert_eq!(class_of(64 << 10), Some(0));
        assert_eq!(class_of((64 << 10) + 1), Some(1));
        assert_eq!(class_of(32 << 20), Some(CLASSES - 1));
        assert_eq!(class_of((32 << 20) + 1), None);
        assert_eq!(class_size(CLASSES - 1), 32 << 20);
    }
}
