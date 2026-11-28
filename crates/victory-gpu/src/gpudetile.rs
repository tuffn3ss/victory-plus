//! Texture detiling on the card (`VICTORY_GPU_DETILE`).
//!
//! Without it a title texture's memory is detiled on the PC into its
//! staging buffer (`crate::texture`, `Staging::detile`): every changed
//! texture is walked element run by element run on the submitting thread
//! and on the threads that detile ahead. With it the staging buffer holds
//! the texture's memory as the title laid it out (one plain copy on the
//! PC), and a shader detiles it on the card into a buffer laid out as the
//! host texture's subresources, from which the list copies as before.
//!
//! What the PC still does per texture: asks the write watch and guards,
//! reads and hashes the memory (change detection is as it was), and copies
//! it into the staging buffer whole. What moves to the card: the address
//! arithmetic of [`victory_formats::texture::Layout::detile`], element by
//! element, for every mode that function handles (linear and the modes
//! without pipe or bank XOR, thin and thick); the shader takes the
//! layout's pattern tables, so no mode has code of its own.
//!
//! Where it runs: in the list's prelude (`HostList::prelude`), the list
//! that runs just before the list at submission, so nothing the title's
//! list has bound is disturbed. It reads the staging buffer as the card
//! finds it when the prelude runs, which is what the PC wrote by then:
//! the same moment the copy of the CPU path read it. The detiled bytes go
//! to a scratch buffer of the prelude's own (one for all the textures it
//! uploads, kept with the list), from which the prelude's copies read: no
//! texture keeps a buffer on the card for it between uploads.
//!
//! Exact: the shader moves the same bytes to the same places (a unit test
//! compares it with the CPU detile byte for byte on the software adapter);
//! levels on demand that are not the title's are zeros, by a mask the PC
//! writes beside the bytes at each detiling.
//!
//! Ported from fxceless' victory-fork 3461f52f. Textures uploaded ahead on
//! the copy queue (`crate::texture::ahead`) keep the PC's way: a copy
//! queue runs no shaders.

use std::sync::OnceLock;

use victory_formats::texture::Layout;
use windows::Win32::Graphics::Direct3D12::*;

/// `VICTORY_GPU_DETILE`: 0 (unset) off; 1 on; `alt` twenty seconds with,
/// twenty without, in turn, each change said in the log. Asked when a
/// texture's staging is made: a texture keeps the way it was made with.
pub(crate) fn on() -> bool {
    /// 0 off, 1 on, 2 in turn.
    static MODE: OnceLock<u8> = OnceLock::new();
    match *MODE.get_or_init(|| match std::env::var("VICTORY_GPU_DETILE").as_deref() {
        Ok("1") => 1,
        Ok("alt") => 2,
        _ => 0,
    }) {
        0 => false,
        1 => true,
        _ => {
            static EPOCH: OnceLock<std::time::Instant> = OnceLock::new();
            let on = (EPOCH.get_or_init(std::time::Instant::now).elapsed().as_secs() / 20) % 2 == 1;
            static LAST: std::sync::atomic::AtomicU8 = std::sync::atomic::AtomicU8::new(2);
            if LAST.swap(u8::from(on), std::sync::atomic::Ordering::Relaxed) != u8::from(on) {
                tracing::info!(
                    "title textures made from now on are detiled {} (VICTORY_GPU_DETILE=alt)",
                    if on { "ON THE CARD" } else { "on the PC, as usual" }
                );
            }
            on
        }
    }
}

/// The root constants of one dispatch (see [`SOURCE`]'s cbuffer).
pub(crate) const CONSTANTS: usize = 22;

/// The shader. One thread writes one 32-bit word of a row of the linear
/// subresource: a word of an element of 4 bytes or more, or the 4 or 2
/// elements of 1 or 2 bytes the word holds. Root SRV: the staging buffer
/// (the tiled bytes, then the mask of held levels, then the pattern
/// tables); root UAV: the linear buffer.
pub(crate) const SOURCE: &str = r#"
#define RS "RootConstants(num32BitConstants=22, b0), SRV(t0), UAV(u0)"
cbuffer P : register(b0) {
    uint flags;       // bit 0: linear
    uint n;           // element bytes: 1, 2, 4, 8, 16
    uint width;       // elements
    uint height;
    uint depth;
    uint first;       // the array slice
    uint slab;        // depth slices a slice of the layout holds
    uint slice_size;
    uint mip_offset;
    uint pitch;       // the mip's padded width in elements
    uint bw;
    uint bh;
    uint log2;        // log2 of the block's bytes
    uint tx;          // the mip's place in the tail block
    uint ty;
    uint tables;      // byte offset of the x table (y, then z, follow)
    uint dst;         // byte offset of the subresource in the linear buffer
    uint row_pitch;
    uint depth_pitch;
    uint words;       // 32-bit words a row of the subresource holds
    uint mask_at;     // byte offset of the held levels' mask
    uint mip;
};
ByteAddressBuffer src : register(t0);
RWByteAddressBuffer out_buf : register(u0);

uint element(uint x, uint y, uint z) {
    uint s = (first + z) / slab;
    uint inner = (first + z) % slab;
    uint base = s * slice_size + mip_offset;
    if ((flags & 1u) != 0u) {
        return base + (y * pitch + x) * n;
    }
    x += tx;
    y += ty;
    uint row = ((y / bh) * (pitch / bw)) << log2;
    uint col = (x / bw) << log2;
    uint py = src.Load(tables + 4u * (bw + (y % bh)));
    uint pz = src.Load(tables + 4u * (bw + bh + inner));
    uint px = src.Load(tables + 4u * (x % bw));
    return base + row + col + (py | pz) + px;
}

[RootSignature(RS)]
[numthreads(64, 1, 1)]
void main(uint3 id : SV_DispatchThreadID) {
    if (id.x >= words || id.y >= height || id.z >= depth) return;
    uint at = dst + id.z * depth_pitch + id.y * row_pitch + id.x * 4u;
    uint held = src.Load(mask_at);
    if (mip < 32u && ((held >> mip) & 1u) == 0u) {
        out_buf.Store(at, 0u);
        return;
    }
    uint v = 0u;
    if (n >= 4u) {
        uint per = n >> 2;
        v = src.Load(element(id.x / per, id.y, id.z) + 4u * (id.x % per));
    } else {
        uint per = 4u / n;
        uint keep = n == 1u ? 0xffu : 0xffffu;
        [unroll]
        for (uint k = 0u; k < 4u; k++) {
            uint x = id.x * per + k;
            if (k < per && x < width) {
                uint a = element(x, id.y, id.z);
                uint w = src.Load(a & ~3u);
                v |= ((w >> ((a & 3u) * 8u)) & keep) << (k * n * 8u);
            }
        }
    }
    out_buf.Store(at, v);
}
"#;

/// The shader's root signature and pipeline.
pub(crate) struct Detiler {
    pub(crate) root: ID3D12RootSignature,
    pub(crate) pso: ID3D12PipelineState,
}

// SAFETY: immutable D3D12 objects, which are free-threaded.
unsafe impl Send for Detiler {}
// SAFETY: as above.
unsafe impl Sync for Detiler {}

/// Compiles [`SOURCE`] (`cs_5_1`, the root signature inside it) on `device`.
pub(crate) fn make(device: &ID3D12Device) -> windows::core::Result<Detiler> {
    use windows::Win32::Graphics::Direct3D::Fxc::D3DCompile;
    use windows::Win32::Graphics::Direct3D::ID3DBlob;
    let mut code: Option<ID3DBlob> = None;
    let mut errors: Option<ID3DBlob> = None;
    // SAFETY: the source and names live through the call; the out
    // pointers are ours.
    let compiled = unsafe {
        D3DCompile(
            SOURCE.as_ptr().cast(),
            SOURCE.len(),
            windows::core::s!("texture detile"),
            None,
            None,
            windows::core::s!("main"),
            windows::core::s!("cs_5_1"),
            0,
            0,
            &mut code,
            Some(&mut errors),
        )
    };
    if let Err(e) = compiled {
        if let Some(errors) = errors {
            // SAFETY: the blob's own bytes.
            let text = unsafe { std::slice::from_raw_parts(errors.GetBufferPointer().cast::<u8>(), errors.GetBufferSize()) };
            tracing::warn!("the texture detile shader: {}", String::from_utf8_lossy(text));
        }
        return Err(e);
    }
    let code = code.ok_or_else(windows::core::Error::empty)?;
    // SAFETY: the blob's own bytes; the root signature is inside the shader.
    unsafe {
        let bytes = std::slice::from_raw_parts(code.GetBufferPointer().cast::<u8>(), code.GetBufferSize());
        let root: ID3D12RootSignature = device.CreateRootSignature(0, bytes)?;
        let pso: ID3D12PipelineState = device.CreateComputePipelineState(&D3D12_COMPUTE_PIPELINE_STATE_DESC {
            pRootSignature: std::mem::transmute_copy(&root),
            CS: D3D12_SHADER_BYTECODE {
                pShaderBytecode: bytes.as_ptr().cast(),
                BytecodeLength: bytes.len(),
            },
            ..Default::default()
        })?;
        Ok(Detiler { root, pso })
    }
}

/// The host's detiler, made at the first ask; `None` (said once) if the
/// shader could not be made: textures are then detiled on the PC.
pub(crate) fn detiler() -> Option<&'static Detiler> {
    static MADE: OnceLock<Option<Detiler>> = OnceLock::new();
    MADE.get_or_init(|| {
        let host = crate::host::host()?;
        match make(&host.device) {
            Ok(d) => {
                tracing::info!("title textures are detiled on the card (VICTORY_GPU_DETILE)");
                Some(d)
            }
            Err(e) => {
                tracing::warn!(error = %e, "VICTORY_GPU_DETILE: the detile shader could not be made; textures are detiled on the PC");
                None
            }
        }
    })
    .as_ref()
}

/// Where things lie in a texture's staging buffer on the card's way: the
/// tiled bytes at 0, the held levels' mask, the pattern tables, and the
/// buffer's bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Raw {
    pub(crate) mask_at: u64,
    pub(crate) tables_at: u64,
    pub(crate) bytes: u64,
}

pub(crate) fn raw_layout(layout: &Layout) -> Raw {
    let mask_at = layout.size.next_multiple_of(16);
    let tables_at = mask_at + 16;
    let words: usize = layout.pattern_tables().iter().map(|t| t.len()).sum();
    Raw {
        mask_at,
        tables_at,
        bytes: tables_at + 4 * words as u64,
    }
}

/// Writes the pattern tables of `layout` and an all-held mask into the
/// staging buffer's mapping `data` ([`raw_layout`]'s places).
///
/// # Safety
/// `data` is the mapping of a buffer of at least `raw.bytes` bytes that
/// nothing else writes now.
pub(crate) unsafe fn write_tables(layout: &Layout, raw: &Raw, data: *mut u8) {
    let mut at = raw.tables_at as usize;
    // SAFETY: inside the buffer, by the caller's word.
    unsafe {
        write_mask(raw, data, u32::MAX);
        for table in layout.pattern_tables() {
            for &v in table {
                data.add(at).cast::<u32>().write_unaligned(v);
                at += 4;
            }
        }
    }
}

/// Writes the held levels' mask (a bit a level; levels past 31 are held).
///
/// # Safety
/// As [`write_tables`].
pub(crate) unsafe fn write_mask(raw: &Raw, data: *mut u8, held: u32) {
    // SAFETY: inside the buffer, by the caller's word.
    unsafe { data.add(raw.mask_at as usize).cast::<u32>().write_unaligned(held) };
}

/// One subresource's dispatch: its constants and its groups.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Job {
    pub(crate) constants: [u32; CONSTANTS],
    pub(crate) groups: [u32; 3],
}

/// The dispatches that detile every subresource of `layout` into a buffer
/// laid out as `footprints` (and `rows`), subresource `i` being mip
/// `first + i % (mips - first)` of slice `i / (mips - first)` (the levels
/// from `first` on, as `crate::texture::Staging` holds them); `None` if one
/// cannot be detiled this way (the texture is then detiled on the PC).
pub(crate) fn jobs(
    layout: &Layout,
    footprints: &[D3D12_PLACED_SUBRESOURCE_FOOTPRINT],
    rows: &[u32],
    first: u32,
    mips: u32,
    raw: &Raw,
) -> Option<Vec<Job>> {
    let tables_at = u32::try_from(raw.tables_at).ok()?;
    let mask_at = u32::try_from(raw.mask_at).ok()?;
    let per = mips.checked_sub(first).filter(|&n| n > 0)?;
    let mut out = Vec::with_capacity(footprints.len());
    for (i, (footprint, &row_count)) in footprints.iter().zip(rows).enumerate() {
        let (mip, slice) = (first + i as u32 % per, i as u32 / per);
        let p = layout.detile_params(mip, slice)?;
        let f = footprint.Footprint;
        let bytes = u64::from(p.width) * u64::from(p.element_bytes);
        let depth_pitch = u64::from(f.RowPitch) * u64::from(row_count);
        let end = footprint.Offset + depth_pitch * u64::from(f.Depth);
        if bytes > u64::from(f.RowPitch)
            || p.height > row_count
            || p.depth > f.Depth
            || end > u64::from(u32::MAX)
            || p.height > 65535
            || p.depth > 65535
        {
            return None;
        }
        let words = bytes.div_ceil(4) as u32;
        out.push(Job {
            constants: [
                u32::from(p.linear),
                p.element_bytes,
                p.width,
                p.height,
                p.depth,
                p.first,
                p.slab,
                p.slice_size,
                p.mip_offset,
                p.pitch,
                p.block_w,
                p.block_h,
                p.block_log2,
                p.tail_x,
                p.tail_y,
                tables_at,
                footprint.Offset as u32,
                f.RowPitch,
                depth_pitch as u32,
                words,
                mask_at,
                mip,
            ],
            groups: [words.div_ceil(64), p.height, p.depth],
        });
    }
    Some(out)
}

/// Records the dispatches that detile the tiled bytes at GPU address `raw`
/// (a staging buffer on the card's way: the bytes, the mask and the tables
/// at [`raw_layout`]'s places) into the buffer at GPU address `linear`, laid
/// out as the footprints the jobs were made for. The caller has the buffer
/// under `linear` in the unordered access state, and moves it on after
/// (with a barrier between these writes and the reads after). Leaves the
/// list's compute root signature and pipeline set to the detiler's.
pub(crate) fn record(list: &ID3D12GraphicsCommandList, d: &Detiler, raw: u64, linear: u64, jobs: &[Job]) {
    // SAFETY: an open list; addresses in live buffers the caller keeps
    // until the list has run; the detiler's own root signature.
    unsafe {
        list.SetComputeRootSignature(&d.root);
        list.SetPipelineState(&d.pso);
        list.SetComputeRootShaderResourceView(1, raw);
        list.SetComputeRootUnorderedAccessView(2, linear);
        for job in jobs {
            list.SetComputeRoot32BitConstants(0, CONSTANTS as u32, job.constants.as_ptr().cast(), 0);
            list.Dispatch(job.groups[0], job.groups[1], job.groups[2]);
        }
    }
}

/// Since the last frame-rate line: textures detiled on the card, their
/// dispatches, the bytes the PC copied into staging buffers whole for
/// them, and the microseconds that copying took; textures whose staging
/// was made for the card's way, and those that could not take it.
pub(crate) static STATS: [std::sync::atomic::AtomicU64; 6] = [const { std::sync::atomic::AtomicU64::new(0) }; 6];

/// Each texture's part of a prelude's scratch buffer begins on a multiple
/// of this: copies read footprints placed on 512 bytes.
pub(crate) const SCRATCH_ALIGN: u64 = 512;

#[cfg(test)]
mod tests {
    use super::*;
    use victory_formats::texture::{Dimension, Swizzle, Texture, TextureDesc};
    use windows::Win32::Foundation::{CloseHandle, HANDLE, WAIT_OBJECT_0};
    use windows::Win32::Graphics::Direct3D::D3D_FEATURE_LEVEL_12_0;
    use windows::Win32::Graphics::Dxgi::Common::*;
    use windows::Win32::System::Threading::{CreateEventW, WaitForSingleObject};

    struct Rig {
        device: ID3D12Device,
        queue: ID3D12CommandQueue,
        allocator: ID3D12CommandAllocator,
        list: ID3D12GraphicsCommandList,
        fence: ID3D12Fence,
        event: HANDLE,
        submitted: u64,
    }

    impl Drop for Rig {
        fn drop(&mut self) {
            // SAFETY: our own event.
            unsafe {
                let _ = CloseHandle(self.event);
            }
        }
    }

    impl Rig {
        /// The software adapter only (never the card).
        fn new() -> Option<Rig> {
            // SAFETY: plain object creation on a device of our own.
            unsafe {
                let adapter = crate::host::warp_adapter().ok()?;
                let mut device: Option<ID3D12Device> = None;
                D3D12CreateDevice(&adapter, D3D_FEATURE_LEVEL_12_0, &mut device).ok()?;
                let device = device?;
                let queue: ID3D12CommandQueue = device
                    .CreateCommandQueue(&D3D12_COMMAND_QUEUE_DESC { Type: D3D12_COMMAND_LIST_TYPE_DIRECT, ..Default::default() })
                    .ok()?;
                let allocator: ID3D12CommandAllocator = device.CreateCommandAllocator(D3D12_COMMAND_LIST_TYPE_DIRECT).ok()?;
                let list: ID3D12GraphicsCommandList =
                    device.CreateCommandList(0, D3D12_COMMAND_LIST_TYPE_DIRECT, &allocator, None).ok()?;
                let fence: ID3D12Fence = device.CreateFence(0, D3D12_FENCE_FLAG_NONE).ok()?;
                let event = CreateEventW(None, false, false, None).ok()?;
                Some(Rig { device, queue, allocator, list, fence, event, submitted: 0 })
            }
        }

        fn run(&mut self, record: impl FnOnce(&ID3D12GraphicsCommandList)) {
            record(&self.list);
            self.submitted += 1;
            // SAFETY: our list, queue and fence; the allocator is reset
            // only after the card finished.
            unsafe {
                self.list.Close().unwrap();
                let run: ID3D12CommandList = windows::core::Interface::cast(&self.list).unwrap();
                self.queue.ExecuteCommandLists(&[Some(run)]);
                self.queue.Signal(&self.fence, self.submitted).unwrap();
                self.fence.SetEventOnCompletion(self.submitted, self.event).unwrap();
                assert_eq!(WaitForSingleObject(self.event, 60_000), WAIT_OBJECT_0, "the software adapter did not finish");
                let removed = self.device.GetDeviceRemovedReason();
                assert!(removed.is_ok(), "device lost: {removed:?}");
                self.allocator.Reset().unwrap();
                self.list.Reset(&self.allocator, None).unwrap();
            }
        }

        fn buffer(&self, kind: D3D12_HEAP_TYPE, flags: D3D12_RESOURCE_FLAGS, state: D3D12_RESOURCE_STATES, bytes: u64) -> ID3D12Resource {
            let heap = D3D12_HEAP_PROPERTIES { Type: kind, ..Default::default() };
            let desc = D3D12_RESOURCE_DESC {
                Dimension: D3D12_RESOURCE_DIMENSION_BUFFER,
                Width: bytes.max(1),
                Height: 1,
                DepthOrArraySize: 1,
                MipLevels: 1,
                SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
                Layout: D3D12_TEXTURE_LAYOUT_ROW_MAJOR,
                Flags: flags,
                ..Default::default()
            };
            let mut out: Option<ID3D12Resource> = None;
            // SAFETY: plain buffer creation.
            unsafe { self.device.CreateCommittedResource(&heap, D3D12_HEAP_FLAG_NONE, &desc, state, None, &mut out).unwrap() };
            out.unwrap()
        }
    }

    fn map(buffer: &ID3D12Resource) -> *mut u8 {
        let mut data = std::ptr::null_mut();
        // SAFETY: a CPU-visible buffer of our own.
        unsafe { buffer.Map(0, None, Some(&mut data)).unwrap() };
        data.cast()
    }

    /// A texture case: console description and the host's.
    struct Case {
        dimension: Dimension,
        width: u32,
        height: u32,
        depth: u32,
        mips: u32,
        format: DXGI_FORMAT,
        swizzle: Swizzle,
    }

    /// The bytes the CPU detile and the card detile leave in a buffer laid
    /// out as the host texture's subresources, for `case`, from the same
    /// pseudo-random tiled bytes; `held` as the mask of held levels (the
    /// CPU's zeros for the others, as `Staging::detile` writes them).
    /// `first`: the staging begins at that level, as one of
    /// `crate::texture::Staging` for a texture read from a smaller level on
    /// (single-slice textures only, as there).
    fn both(rig: &mut Rig, d: &Detiler, case: &Case, held: u32, first: u32) -> bool {
        let texture = Texture::new(&TextureDesc {
            dimension: case.dimension,
            width: case.width,
            height: case.height,
            depth_or_array: case.depth,
            mips: case.mips,
            format: case.format.0 as u32,
            samples: 1,
            swizzle: case.swizzle,
        })
        .unwrap();
        let layout = texture.planes[0].layout.clone();
        let three_d = case.dimension == Dimension::Tex3d;
        let host_desc = D3D12_RESOURCE_DESC {
            Dimension: if three_d { D3D12_RESOURCE_DIMENSION_TEXTURE3D } else { D3D12_RESOURCE_DIMENSION_TEXTURE2D },
            Width: u64::from(case.width),
            Height: case.height,
            DepthOrArraySize: case.depth as u16,
            MipLevels: case.mips as u16,
            Format: case.format,
            SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
            ..Default::default()
        };
        let per = case.mips - first;
        let count = if three_d { per } else { per * case.depth };
        let mut footprints = vec![D3D12_PLACED_SUBRESOURCE_FOOTPRINT::default(); count as usize];
        let mut rows = vec![0u32; count as usize];
        let mut total = 0u64;
        // SAFETY: outputs for `count` subresources.
        unsafe {
            rig.device.GetCopyableFootprints(&host_desc, first, count, 0, Some(footprints.as_mut_ptr()), Some(rows.as_mut_ptr()), None, Some(&mut total));
        }
        // The title's bytes.
        let mut seed = 0x9E37_79B9_7F4A_7C15u64 ^ u64::from(case.width * 31 + case.height);
        let tiled: Vec<u8> = (0..layout.size)
            .map(|_| {
                seed ^= seed << 13;
                seed ^= seed >> 7;
                seed ^= seed << 17;
                (seed >> 24) as u8
            })
            .collect();
        // The CPU's way, as `Staging::detile`.
        // (A last row's padding lies past `total`: room for the CPU's fills.)
        let mut cpu = vec![0xA5u8; total as usize + 4096];
        for (i, (footprint, &r)) in footprints.iter().zip(&rows).enumerate() {
            let (mip, slice) = (first + i as u32 % per, i as u32 / per);
            let f = footprint.Footprint;
            let pitch = f.RowPitch as usize;
            let len = pitch * r as usize * f.Depth as usize;
            let linear = &mut cpu[footprint.Offset as usize..][..len];
            if mip < 32 && held & (1 << mip) == 0 {
                linear.fill(0);
                continue;
            }
            layout.detile(&tiled, mip, slice, linear, pitch, pitch * r as usize).unwrap();
        }
        // The card's way.
        let raw = raw_layout(&layout);
        let jobs = jobs(&layout, &footprints, &rows, first, case.mips, &raw).expect("detilable on the card");
        let upload = rig.buffer(D3D12_HEAP_TYPE_UPLOAD, D3D12_RESOURCE_FLAG_NONE, D3D12_RESOURCE_STATE_GENERIC_READ, raw.bytes);
        let data = map(&upload);
        // SAFETY: the mapping holds `raw.bytes`.
        unsafe {
            std::ptr::copy_nonoverlapping(tiled.as_ptr(), data, tiled.len());
            write_tables(&layout, &raw, data);
            write_mask(&raw, data, held);
        }
        let linear = rig.buffer(D3D12_HEAP_TYPE_DEFAULT, D3D12_RESOURCE_FLAG_ALLOW_UNORDERED_ACCESS, D3D12_RESOURCE_STATE_COMMON, total);
        let back = rig.buffer(D3D12_HEAP_TYPE_READBACK, D3D12_RESOURCE_FLAG_NONE, D3D12_RESOURCE_STATE_COPY_DEST, total);
        rig.run(|list| {
            use crate::host::transition;
            // SAFETY: an open list; the buffers outlive the run.
            unsafe {
                list.ResourceBarrier(&[transition(&linear, D3D12_RESOURCE_STATE_COMMON, D3D12_RESOURCE_STATE_UNORDERED_ACCESS)]);
                record(list, d, upload.GetGPUVirtualAddress(), linear.GetGPUVirtualAddress(), &jobs);
                list.ResourceBarrier(&[transition(&linear, D3D12_RESOURCE_STATE_UNORDERED_ACCESS, D3D12_RESOURCE_STATE_COPY_SOURCE)]);
                list.CopyBufferRegion(&back, 0, &linear, 0, total);
            }
        });
        let read = map(&back);
        // SAFETY: the read-back buffer holds `total` bytes the list wrote.
        let card = unsafe { std::slice::from_raw_parts(read.cast_const(), total as usize) };
        // A row's own bytes must agree (the rest of a pitch is padding the
        // copy into the texture never reads).
        let mut same = true;
        for (i, (footprint, &r)) in footprints.iter().zip(&rows).enumerate() {
            let mip = first + i as u32 % per;
            let m = &layout.mips[mip as usize];
            let n = layout.desc.element_bytes as usize;
            let f = footprint.Footprint;
            for z in 0..m.depth as usize {
                for y in 0..m.height as usize {
                    let at = footprint.Offset as usize + z * f.RowPitch as usize * r as usize + y * f.RowPitch as usize;
                    let own = m.width as usize * n;
                    if cpu[at..at + own] != card[at..at + own] {
                        if same {
                            eprintln!(
                                "{:?} {}x{}x{} {:?}: subresource {i} (mip {mip}) row {y} depth {z} differs",
                                case.swizzle, case.width, case.height, case.depth, case.format
                            );
                        }
                        same = false;
                    }
                }
            }
        }
        same
    }

    /// The card detiles every addressable mode as the CPU does, byte for
    /// byte, on the software adapter: BC and plain formats of every element
    /// size, mip tails, arrays, thick 3D, linear, and levels on demand that
    /// are not the title's.
    #[test]
    fn the_card_detiles_as_the_cpu_does() {
        let Some(mut rig) = Rig::new() else {
            eprintln!("no software adapter: not run");
            return;
        };
        let d = make(&rig.device).expect("the detile shader compiles");
        let t2 = |width, height, mips, format, swizzle| Case { dimension: Dimension::Tex2d, width, height, depth: 1, mips, format, swizzle };
        let cases = [
            t2(1024, 1024, 11, DXGI_FORMAT_BC7_UNORM, Swizzle::SW_64KB_S),
            t2(512, 256, 10, DXGI_FORMAT_BC1_UNORM, Swizzle::SW_64KB_S),
            t2(256, 256, 9, DXGI_FORMAT_BC3_UNORM, Swizzle::SW_4KB_S),
            t2(100, 60, 7, DXGI_FORMAT_BC5_UNORM, Swizzle::SW_4KB_S),
            t2(300, 200, 9, DXGI_FORMAT_R8G8B8A8_UNORM, Swizzle::SW_64KB_S),
            t2(129, 65, 8, DXGI_FORMAT_R8G8B8A8_UNORM, Swizzle::SW_4KB_S),
            t2(97, 61, 7, DXGI_FORMAT_R8_UNORM, Swizzle::SW_64KB_S),
            t2(77, 33, 7, DXGI_FORMAT_R16_UNORM, Swizzle::SW_4KB_D),
            t2(64, 64, 7, DXGI_FORMAT_R16G16B16A16_FLOAT, Swizzle::SW_64KB_D),
            t2(50, 30, 6, DXGI_FORMAT_R32G32B32A32_FLOAT, Swizzle::SW_256B_S),
            t2(33, 17, 6, DXGI_FORMAT_R8_UNORM, Swizzle::SW_256B_D),
            t2(130, 70, 8, DXGI_FORMAT_R8G8B8A8_UNORM, Swizzle::LINEAR),
            t2(61, 3, 6, DXGI_FORMAT_R8_UNORM, Swizzle::LINEAR),
            Case { depth: 6, ..t2(128, 128, 8, DXGI_FORMAT_BC1_UNORM, Swizzle::SW_4KB_S) },
            Case { depth: 3, ..t2(70, 70, 4, DXGI_FORMAT_R8G8B8A8_UNORM, Swizzle::SW_64KB_S) },
            Case { dimension: Dimension::Tex3d, depth: 16, ..t2(64, 32, 7, DXGI_FORMAT_R8G8B8A8_UNORM, Swizzle::SW_4KB_S) },
            Case { dimension: Dimension::Tex3d, depth: 40, ..t2(33, 20, 6, DXGI_FORMAT_R16_UNORM, Swizzle::SW_64KB_S) },
        ];
        let mut all = true;
        for case in &cases {
            all &= both(&mut rig, &d, case, u32::MAX, 0);
        }
        // Levels on demand: levels 0 and 1 not the title's (zeros), the rest held.
        all &= both(&mut rig, &d, &cases[0], !0b11, 0);
        all &= both(&mut rig, &d, &cases[4], !0b1, 0);
        // Stagings that begin at a smaller level (this tree's `held_only`).
        all &= both(&mut rig, &d, &cases[0], u32::MAX, 2);
        all &= both(&mut rig, &d, &cases[4], u32::MAX, 3);
        all &= both(&mut rig, &d, &cases[7], !0b1000, 1);
        assert!(all, "the card's detiling differs from the CPU's (see above)");
    }

    /// The costs this replaces, on this PC (run alone, in release:
    /// `cargo test --release -p victory-gpu --lib gpudetile::tests::cpu_costs -- --ignored --nocapture`).
    #[test]
    #[ignore = "a timing, not a check"]
    fn cpu_costs() {
        for (width, mips, format, swizzle) in [
            (2048u32, 12u32, DXGI_FORMAT_BC7_UNORM, Swizzle::SW_64KB_S),
            (2048, 12, DXGI_FORMAT_BC1_UNORM, Swizzle::SW_64KB_S),
            (1024, 11, DXGI_FORMAT_R8G8B8A8_UNORM, Swizzle::SW_64KB_S),
            (512, 10, DXGI_FORMAT_BC7_UNORM, Swizzle::SW_4KB_S),
            (1024, 11, DXGI_FORMAT_R8G8B8A8_UNORM, Swizzle::SW_4KB_S),
        ] {
            let texture = Texture::new(&TextureDesc {
                dimension: Dimension::Tex2d,
                width,
                height: width,
                depth_or_array: 1,
                mips,
                format: format.0 as u32,
                samples: 1,
                swizzle,
            })
            .unwrap();
            let layout = &texture.planes[0].layout;
            // Copies of the title's bytes past the CPU's caches (some 200
            // MB), read in turn: cold, as title memory is.
            let copies = ((200usize << 20) / layout.size as usize).clamp(2, 64);
            let tiled: Vec<Vec<u8>> = (0..copies).map(|c| (0..layout.size).map(|i| (i * 7 + c as u64) as u8).collect()).collect();
            // Linear rows of 256-byte aligned pitch, subresources 512 aligned.
            let mut places = vec![];
            let mut total = 0usize;
            for m in &layout.mips {
                let pitch = (m.width as usize * layout.desc.element_bytes as usize).next_multiple_of(256);
                places.push((total, pitch, m.height as usize));
                total = (total + pitch * m.height as usize).next_multiple_of(512);
            }
            // Destinations in write-combined memory, as an upload heap's
            // mapping is on a card.
            let wc = |bytes: usize| -> &'static mut [u8] {
                use windows_sys::Win32::System::Memory::*;
                // SAFETY: a fresh allocation of our own, never freed (a test).
                unsafe {
                    let at = VirtualAlloc(std::ptr::null(), bytes, MEM_COMMIT | MEM_RESERVE, PAGE_READWRITE | PAGE_WRITECOMBINE);
                    assert!(!at.is_null());
                    std::slice::from_raw_parts_mut(at.cast::<u8>(), bytes)
                }
            };
            let linear = wc(total + 4096);
            let raw = wc(layout.size as usize);
            let time = |f: &mut dyn FnMut(&[u8])| {
                let started = std::time::Instant::now();
                for t in &tiled {
                    f(t);
                }
                started.elapsed().as_secs_f64() / copies as f64
            };
            let detile = time(&mut |t| {
                for (mip, &(at, pitch, h)) in places.iter().enumerate() {
                    layout.detile(t, mip as u32, 0, &mut linear[at..], pitch, pitch * h).unwrap();
                }
            });
            let copy = time(&mut |t| {
                raw.copy_from_slice(t);
                std::hint::black_box(&raw);
            });
            let hash = time(&mut |t| {
                std::hint::black_box(blake3::hash(t));
            });
            let mb = layout.size as f64 / 1e6;
            eprintln!(
                "{swizzle:?} {width}x{width} {format:?}: {mb:.1} MB, cold, into write-combined memory; detile {:.2} ms ({:.1} GB/s), plain copy {:.2} ms ({:.1} GB/s), blake3 {:.2} ms ({:.1} GB/s)",
                detile * 1e3,
                mb / detile / 1e3,
                copy * 1e3,
                mb / copy / 1e3,
                hash * 1e3,
                mb / hash / 1e3
            );
        }
    }
}
