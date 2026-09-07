//! ต่อสายฝั่ง GPU เข้ากับฝั่งตัวเข้ารหัส — งาน export หนึ่งงานทั้งเส้น (P5-4)
//!
//! ★ crate นี้เป็นที่เดียวที่รู้จักทั้ง `refx-render` และ `refx-asset` จึงเป็นที่
//! เดียวที่ประกอบสองฝั่งเข้าด้วยกันได้ · รูปแบบเดียวกับที่ `refx-ui` เป็นคนส่ง
//! `rename_durable` ให้ `refx-io` (ดู `refx_io::save::RenameFn`)
//!
//! ## I-2 — ห้ามเรียกจาก UI thread
//!
//! [`export_board`] **บล็อกรอ GPU** ทุกแถบ และเข้ารหัสทั้งไฟล์ในเธรดที่เรียก
//! `docs/07 §6` ข้อ 1 บังคับให้ทำงานนี้อยู่บน worker · ผู้เรียกต้องยิงมันจาก
//! เธรดของตัวเองแล้วส่งผลกลับมาทางช่อง (จะทำพร้อม dialog ใน P5-4 รอบถัดไป)
//!
//! ## เพดาน RAM
//!
//! `docs/07 §6` บังคับว่าต้องพิสูจน์ด้วย **RSS จริง** ไม่ใช่ผลรวมบนกระดาษ —
//! ดูเทสต์ [`tests::what_an_export_really_costs_in_process_memory`]
//!
//! spec: docs/07-file-format.md §6

use std::path::PathBuf;
use std::sync::atomic::AtomicBool;

use refx_asset::export::{BandError, BandSource, ExportFormat, ExportStats, RenameFn};
use refx_core::export::BandPlan;
use refx_core::geom::Rect;
use refx_render::export::BandRenderer;
use refx_render::pipeline::DrawBatch;
use refx_render::texture::TextureAllocator;

/// สิ่งที่ผู้ใช้เลือกในกล่อง export
#[derive(Debug, Clone, PartialEq)]
pub struct ExportRequest {
    /// ไฟล์ปลายทาง — ผู้เรียกต้องผ่าน `validate_asset_path()` มาก่อนแล้ว
    pub path: PathBuf,
    /// รูปแบบไฟล์
    pub format: ExportFormat,
    /// ขนาดภาพปลายทาง (พิกเซล)
    pub size: (u32, u32),
    /// กรอบใน world ที่จะถูก export (ปกติคือขอบเขตของทั้ง board)
    pub region: Rect,
    /// สีพื้นหลัง sRGB
    ///
    /// ★ JPEG ไม่มี alpha → ผู้เรียก**ต้อง**ส่ง alpha 255 มา ไม่งั้นภาพจะได้
    /// พื้นดำโดยไม่ได้ตั้งใจ (`docs/07 §6`) · [`export_board`] บังคับข้อนี้ให้
    pub background: [u8; 4],
}

/// export ไม่สำเร็จ
#[derive(Debug, thiserror::Error)]
pub enum ExportJobError {
    /// ★ ไม่มีอะไรให้วาด — **ปฏิเสธ ห้ามเขียนไฟล์ 0 ไบต์** (`docs/07 §6` ข้อ 5)
    #[error("there is nothing on this board to export")]
    NothingToExport,
    /// ขนาดที่ขอ export ไม่ได้
    #[error(transparent)]
    Plan(#[from] refx_core::export::PlanError),
    /// เตรียมเป้าบน GPU ไม่ได้
    #[error(transparent)]
    Render(#[from] refx_render::export::ExportRenderError),
    /// เขียนไฟล์ไม่ได้ / ถูกยกเลิก
    #[error(transparent)]
    Write(#[from] refx_asset::export::ExportError),
}

/// ต้นทางพิกเซลที่วาดจาก GPU ทีละแถบ
///
/// ★ ทำหน้าที่เดียว: แปลง "ขอแถวที่ `y0`" เป็น "วาดแถบที่ `index`" ·
/// ตัวมันเอง**ไม่ถือบัฟเฟอร์แถบ** — เขียนลงบัฟเฟอร์ของตัวเข้ารหัสตรง ๆ
/// (เหตุผลเต็มอยู่ที่ `BandRenderer::render_band`)
struct GpuBands<'a> {
    renderer: BandRenderer,
    device: &'a wgpu::Device,
    queue: &'a wgpu::Queue,
    batches: &'a [DrawBatch<'a>],
}

impl BandSource for GpuBands<'_> {
    fn fill(&mut self, y0: u32, rows: u32, out: &mut [u8]) -> Result<(), BandError> {
        let band_rows = self.renderer.plan().band_rows();
        // ★ ตัวเข้ารหัสขอเป็น "แถวที่เท่าไหร่" ส่วนเราวาดเป็น "แถบที่เท่าไหร่" —
        //   ถ้าสองอย่างนี้ไม่ตรงกันเป๊ะ ภาพจะเหลื่อมทีละแถบโดยไม่มี error
        //   จึงตรวจซ้ำแทนที่จะเชื่อ (ดู `docs/08 §3.9` ข้อ 8)
        if !y0.is_multiple_of(band_rows) {
            return Err(BandError(format!(
                "row {y0} is not the start of a band of {band_rows} rows"
            )));
        }
        let written = self
            .renderer
            .render_band(self.device, self.queue, y0 / band_rows, self.batches, out)
            .map_err(|err| BandError(err.to_string()))?;
        if written != rows {
            return Err(BandError(format!(
                "the renderer produced {written} rows where {rows} were asked for at row {y0}"
            )));
        }
        Ok(())
    }
}

/// จำนวน instance ทั้งหมดในทุกก้อน — ศูนย์ = ไม่มีอะไรให้ export
#[must_use]
pub fn instances_in(batches: &[DrawBatch<'_>]) -> usize {
    batches.iter().map(|batch| batch.instances.len()).sum()
}

/// ของบน GPU ที่ export ต้องใช้ — มัดไว้ก้อนเดียวเพราะทั้งสี่ตัวมาจากที่เดียวกัน
/// (`RenderContext` + atlas ของแอป) และเดินทางไปด้วยกันเสมอ
#[derive(Clone, Copy)]
pub struct GpuAccess<'a> {
    /// device ที่ทุกอย่างผูกอยู่
    pub device: &'a wgpu::Device,
    /// คิวคำสั่ง
    pub queue: &'a wgpu::Queue,
    /// ทางเดียวที่จอง texture ได้ (I-6)
    pub allocator: &'a TextureAllocator,
    /// layout ของ bind group ที่ shader ใช้อ่าน texture
    pub atlas_layout: &'a wgpu::BindGroupLayout,
}

/// เขียนไฟล์ export ของ board หนึ่งใบ — **ต้องเรียกจาก worker ไม่ใช่ UI thread**
///
/// # Errors
/// [`ExportJobError`] — board ว่าง, ขนาดเกินเพดาน, GPU ล้ม, ถูกยกเลิก
/// หรือระบบไฟล์ล้ม · **ไฟล์ปลายทางเดิม (ถ้ามี) ยังอยู่ครบเสมอ**
pub fn export_board(
    gpu: GpuAccess<'_>,
    batches: &[DrawBatch<'_>],
    request: &ExportRequest,
    cancel: &AtomicBool,
    rename: RenameFn,
) -> Result<ExportStats, ExportJobError> {
    // ★★ ปฏิเสธ**ก่อน**แตะดิสก์ — ไฟล์ 0 ไบต์ที่ชื่อเหมือนงานของผู้ใช้
    //    อ่านได้ว่า "โปรแกรมทำงานหาย" ซึ่งแย่กว่าการบอกตรง ๆ ว่าไม่มีอะไรให้เขียน
    if instances_in(batches) == 0 {
        return Err(ExportJobError::NothingToExport);
    }

    // ★★★ **สเปกสองที่ขัดกันตรงนี้ — ยังไม่เรียก `validate_asset_path()`**
    //
    // `docs/07 §6` ข้อ 4 เขียนว่า path ของ export ต้องผ่านด่านนั้น
    // แต่ `refx_io::validate` (docs/06 §4) เขียนไว้ที่หัวโมดูลตัวเองว่า
    //   *"เรียกทุกจุดที่ path มาจาก **ไฟล์** ไม่ใช่จากการที่ผู้ใช้เลือกเอง —
    //     ผู้ใช้ที่กด 'หาไฟล์เอง' แล้วชี้ไปที่ไดรฟ์เครือข่าย **ตั้งใจทำแบบนั้น**"*
    //
    // path ของ export มาจากกล่องบันทึกไฟล์ = ผู้ใช้เลือกเอง · ถ้าเรียกด่านนั้น
    // การ export ลงไดรฟ์ที่แชร์ไว้ (งานประจำของนักวาดที่ทำงานเป็นทีม) จะถูก
    // ปฏิเสธทั้งที่ผู้ใช้ตั้งใจ — เป็นการตัดฟีเจอร์แบบเงียบ ๆ
    //
    // → **ไม่เดา** ปล่อยไว้ให้เจ้าของสเปกตัดสินพร้อมกล่อง export รอบหน้า
    //   (CLAUDE.md: "เจอสิ่งที่ spec ขัดกันเอง → หยุด ถาม ไม่ต้องเดา")

    let plan = BandPlan::new(request.size.0, request.size.1)?;

    // ★★★ JPEG ไม่มี alpha — บังคับทึบที่นี่ **ไม่ใช่หวังว่าผู้เรียกจำได้**
    //     พลาดข้อนี้ = ผู้ใช้ได้ภาพพื้นดำโดยไม่ได้ตั้งใจ (`docs/07 §6`)
    let mut background = request.background;
    if matches!(request.format, ExportFormat::Jpeg { .. }) {
        background[3] = 255;
    }

    let renderer = BandRenderer::new(
        gpu.device,
        gpu.allocator,
        gpu.atlas_layout,
        plan,
        request.region,
        background,
    )?;
    let mut source = GpuBands {
        renderer,
        device: gpu.device,
        queue: gpu.queue,
        batches,
    };

    let stats = refx_asset::export::export_to_file(
        &request.path,
        request.format,
        plan,
        &mut source,
        cancel,
        rename,
    )?;
    Ok(stats)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use std::sync::Arc;
    use std::sync::atomic::Ordering;

    use refx_core::export::PEAK_CEILING;
    use refx_core::glam::Vec2;
    use refx_render::atlas::ThumbnailAtlas;
    use refx_render::instance::{QuadInstance, pack_tint};

    use super::*;

    /// GPU + atlas ที่มีพิกเซลจริงอยู่หนึ่งช่อง
    fn scene() -> Option<(
        wgpu::Device,
        wgpu::Queue,
        TextureAllocator,
        ThumbnailAtlas,
        wgpu::Instance,
    )> {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
        // ★ ไม่มีการ์ดจริงก็ยังเทสต์ได้ — ขอ software adapter แทน (เหมือน
        //   `refx_render::device::headless_device` ซึ่งเป็น `#[cfg(test)]`
        //   ของ crate นั้น จึงเรียกจากที่นี่ไม่ได้)
        let ask = |fallback| {
            pollster_lite(instance.request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::LowPower,
                compatible_surface: None,
                force_fallback_adapter: fallback,
            }))
            .and_then(Result::ok)
        };
        let adapter = ask(false).or_else(|| ask(true))?;
        let info = adapter.get_info();
        let limits = adapter.limits();
        let (device, queue) = pollster_lite(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("refx-export-test"),
            required_features: wgpu::Features::empty(),
            required_limits: limits,
            ..Default::default()
        }))?
        .ok()?;

        let caps = refx_render::device::GpuCapabilities {
            adapter_name: info.name.clone(),
            backend: info.backend,
            device_type: info.device_type,
            bc_compression: false,
            max_texture_dimension_2d: device.limits().max_texture_dimension_2d,
        };
        let allocator = TextureAllocator::new(&caps);
        let mut atlas = ThumbnailAtlas::new(&device, &allocator, 4).ok()?;
        atlas.resize(&device, 1).ok()?;
        Some((device, queue, allocator, atlas, instance))
    }

    /// รอ future ที่ไม่มี waker จริง — พอสำหรับ `request_adapter`/`request_device`
    /// ซึ่ง wgpu ทำเสร็จทันทีบน backend ที่เราใช้ (เหมือน `block_on` ของ `device.rs`)
    fn pollster_lite<F: std::future::Future>(future: F) -> Option<F::Output> {
        use std::task::{Context, Poll, Waker};
        let mut future = Box::pin(future);
        let mut context = Context::from_waker(Waker::noop());
        for _ in 0..10_000 {
            if let Poll::Ready(value) = future.as_mut().poll(&mut context) {
                return Some(value);
            }
            std::thread::yield_now();
        }
        None
    }

    fn quads(region: Rect, count: u32) -> Vec<QuadInstance> {
        let size = region.size();
        (0..count)
            .map(|i| {
                let t = i as f32 / count as f32;
                QuadInstance {
                    transform: [
                        size.x * 0.4,
                        0.0,
                        0.0,
                        size.y * 0.4,
                        region.min.x + size.x * t * 0.5,
                        region.min.y + size.y * t * 0.5,
                    ],
                    uv_rect: [0.0, 0.0, 1.0, 1.0],
                    tint: pack_tint([t, 1.0 - t, 0.5, 1.0]),
                    layer: 0,
                    flags: refx_render::instance::flags::PLACEHOLDER,
                    adjust: QuadInstance::NEUTRAL_ADJUST,
                    reserved: 0,
                }
            })
            .collect()
    }

    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "refx-uiexport-{tag}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn plain_rename(from: &std::path::Path, to: &std::path::Path) -> std::io::Result<()> {
        std::fs::rename(from, to)
    }

    /// ★★★ **board ว่าง = ปฏิเสธ ไม่ใช่ไฟล์ 0 ไบต์** (`docs/07 §6` ข้อ 5)
    ///
    /// ข้อนี้ไม่ต้องใช้ GPU เลย เพราะการปฏิเสธต้องเกิด**ก่อน**จองอะไรทั้งสิ้น
    #[test]
    fn an_empty_board_is_refused_before_anything_is_allocated() {
        let Some((device, queue, allocator, atlas, _instance)) = scene() else {
            eprintln!("ข้าม: ไม่มี GPU adapter");
            return;
        };
        let dir = temp_dir("empty");
        let path = dir.join("empty.png");

        // ★ ก้อนที่มีอยู่แต่ว่างเปล่าก็ยังนับเป็นว่าง — เป็นสภาพที่เกิดจริงเมื่อ
        //   ทุกใบบน board เป็นโน้ตข้อความ (egui วาด ไม่ใช่ quad)
        let empty = [DrawBatch {
            bind_group: atlas.bind_group(),
            instances: &[],
        }];
        for batches in [&empty[..], &[][..]] {
            let err = export_board(
                GpuAccess {
                    device: &device,
                    queue: &queue,
                    allocator: &allocator,
                    atlas_layout: atlas.bind_group_layout(),
                },
                batches,
                &ExportRequest {
                    path: path.clone(),
                    format: ExportFormat::Png { transparent: true },
                    size: (256, 256),
                    region: Rect::from_corners(Vec2::ZERO, Vec2::new(256.0, 256.0)),
                    background: [0, 0, 0, 0],
                },
                &AtomicBool::new(false),
                plain_rename,
            )
            .unwrap_err();
            assert!(matches!(err, ExportJobError::NothingToExport), "ได้ {err:?}");
        }
        assert!(!path.exists(), "เขียนไฟล์ทั้งที่ไม่มีอะไรให้ export");
    }

    /// ★★★ **negative control ของการตัดสินใจ "tile ทรงแถบ ไม่ใช่จตุรัส"**
    ///
    /// สเปกรุ่นแรกเขียนว่า render ทีละ 4096×4096 · ข้อนี้พิสูจน์ว่าทางนั้น
    /// **พังจริง**: บัฟเฟอร์อ่านกลับของ tile จตุรัสใบเดียวกิน RSS มากกว่าเพดาน
    /// ของ export ทั้งงาน · ถ้าไม่มีข้อนี้ เราจะมีแค่คำอธิบายว่าทำไมถึงเลือก
    /// ทรงแถบ ไม่มีหลักฐาน (`docs/08 §3.9` ข้อ 1)
    #[test]
    fn a_square_tile_readback_really_does_blow_the_ceiling() {
        let Some((device, _queue, _allocator, _atlas, _instance)) = scene() else {
            eprintln!("ข้าม: ไม่มี GPU adapter");
            return;
        };
        let Some(before) = refx_platform::memory::process_memory() else {
            eprintln!("ข้าม: แพลตฟอร์มนี้ยังตอบ RSS ไม่ได้");
            return;
        };

        // tile จตุรัส 4096² RGBA — บัฟเฟอร์อ่านกลับ **ใบเดียว** ของทางที่ไม่ได้เลือก
        let square = u64::from(refx_core::export::TILE_WIDTH)
            * u64::from(refx_core::export::TILE_WIDTH)
            * u64::from(refx_core::export::BYTES_PER_PIXEL);
        let buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("refx-nc-square-tile"),
            size: square,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: true,
        });
        // ★ เขียนลงไปจริงเพื่อให้มันเข้า working set ไม่ใช่แค่ถูกจองไว้เฉย ๆ
        //   (`BufferViewMut` เขียนได้อย่างเดียว อ่านไม่ได้ — หน่วยความจำที่ map
        //    มาอาจเป็นแบบ write-combining ซึ่งการอ่านกลับช้ามากจน wgpu กันไว้)
        {
            let mut mapped = buffer.slice(..).get_mapped_range_mut();
            let page = vec![1u8; 1 << 20];
            for chunk in 0..(square / (1 << 20)) {
                let at = chunk * (1 << 20);
                mapped
                    .slice(at as usize..(at as usize + page.len()))
                    .copy_from_slice(&page);
            }
        }
        let after = refx_platform::memory::process_memory().unwrap().current;
        let growth = after.saturating_sub(before.current);

        // ทางที่ไม่ได้เลือกต้องถือ **ทั้งสองอย่างพร้อมกัน**: บัฟเฟอร์อ่านกลับของ
        // tile จตุรัส และบัฟเฟอร์แถบที่ป้อนตัวเข้ารหัส
        let square_design = growth + refx_core::export::BAND_BUDGET as u64;
        println!(
            "NC วิ่งผ่านจริง: บัฟเฟอร์อ่านกลับของ tile จตุรัส 4096² = {} ไบต์ \
             → RSS +{} ไบต์ · บวกบัฟเฟอร์แถบอีก {} MB = {} MB ซึ่งเกินเพดาน {} MB",
            square,
            growth,
            refx_core::export::BAND_BUDGET >> 20,
            square_design >> 20,
            PEAK_CEILING >> 20
        );
        assert!(
            growth >= square * 3 / 4,
            "NC ไม่แดง — บัฟเฟอร์ {square} ไบต์ควรเข้า working set จริง แต่ RSS ขยับแค่ {growth} \
             ถ้าวัดไม่เจอ ตัวเลข RSS ของเทสต์ข้างล่างก็เชื่อไม่ได้เหมือนกัน"
        );
        assert!(
            square_design > PEAK_CEILING as u64,
            "NC ไม่แดง — ทาง tile จตุรัสควรเกินเพดาน แต่รวมแล้วได้แค่ {} MB \
             ถ้าเป็นแบบนั้นจริง เหตุผลที่เลือกทรงแถบก็ไม่มีหลักฐานรองรับ",
            square_design >> 20
        );
        buffer.unmap();
    }

    /// ★★★ **ราคาจริงของ export ในหน่วยความจำของโปรเซส**
    ///
    /// `docs/07 §6` บังคับข้อนี้ไว้ตรง ๆ: ตัวเลขที่วัดไว้ก่อนหน้าเป็นฝั่ง CPU ล้วน
    /// **ทางอ่านกลับจาก GPU ยังไม่เคยถูกวัด** — และนั่นคือที่ที่เพดานจะพังจริง
    /// เพราะ tile จตุรัส 4096² = 67 MB ใหญ่กว่าบัฟเฟอร์แถบทั้งก้อน
    ///
    /// ★★ วัดด้วย **เธรดที่คอยอ่าน RSS ระหว่างทาง** ไม่ใช่ค่าก่อน–หลัง
    /// ยอดดอยเกิดกลางทางแล้วหายไปก่อนเราจะอ่านค่าหลังเสร็จ
    #[test]
    fn what_an_export_really_costs_in_process_memory() {
        let Some((device, queue, allocator, atlas, _instance)) = scene() else {
            eprintln!("ข้าม: ไม่มี GPU adapter");
            return;
        };
        if refx_platform::memory::process_memory().is_none() {
            eprintln!("ข้าม: แพลตฟอร์มนี้ยังตอบ RSS ไม่ได้");
            return;
        }
        let dir = temp_dir("rss");

        let run = |side: u32, format: ExportFormat, label: &str| -> u64 {
            let region = Rect::from_corners(Vec2::ZERO, Vec2::new(side as f32, side as f32));
            let instances = quads(region, 64);
            let batches = [DrawBatch {
                bind_group: atlas.bind_group(),
                instances: &instances,
            }];
            let path = dir.join(format!("{label}-{side}.{}", format.extension()));
            let request = ExportRequest {
                path: path.clone(),
                format,
                size: (side, side),
                region,
                background: [24, 24, 28, 255],
            };

            let stop = Arc::new(AtomicBool::new(false));
            let peak = Arc::new(std::sync::atomic::AtomicU64::new(0));
            let sampler = {
                let (stop, peak) = (Arc::clone(&stop), Arc::clone(&peak));
                std::thread::spawn(move || {
                    while !stop.load(Ordering::Relaxed) {
                        if let Some(mem) = refx_platform::memory::process_memory() {
                            peak.fetch_max(mem.current, Ordering::Relaxed);
                        }
                        std::thread::sleep(std::time::Duration::from_millis(2));
                    }
                })
            };

            let before = refx_platform::memory::process_memory().unwrap().current;
            peak.fetch_max(before, Ordering::Relaxed);
            let start = std::time::Instant::now();
            let stats = export_board(
                GpuAccess {
                    device: &device,
                    queue: &queue,
                    allocator: &allocator,
                    atlas_layout: atlas.bind_group_layout(),
                },
                &batches,
                &request,
                &AtomicBool::new(false),
                plain_rename,
            )
            .unwrap();
            let elapsed = start.elapsed();

            stop.store(true, Ordering::Relaxed);
            let _ = sampler.join();
            let top = peak.load(Ordering::Relaxed);
            let growth = top.saturating_sub(before);

            println!(
                "{label} {side}²: RSS {} MB → {} MB (+{} MB) · ไฟล์ {} MB · {} แถบ · {elapsed:?}",
                before >> 20,
                top >> 20,
                growth >> 20,
                stats.bytes >> 20,
                stats.bands
            );
            let _ = std::fs::remove_file(&path);
            growth
        };

        // ★ รอบอุ่นเครื่อง — ครั้งแรกรวมค่าเปิด pipeline/ตัวจัดสรรของ wgpu ไว้ด้วย
        //   ซึ่งไม่ใช่ราคาของ *ขนาด* ที่ผู้ใช้เลือก
        run(1024, ExportFormat::Png { transparent: false }, "อุ่นเครื่อง");

        let mut worst = 0u64;
        for format in [
            ExportFormat::Png { transparent: false },
            ExportFormat::Jpeg { quality: 90 },
        ] {
            let label = format.name();
            let at_4k = run(4096, format, label);
            let at_8k = run(8192, format, label);
            let at_16k = run(16384, format, label);
            worst = worst.max(at_4k).max(at_8k).max(at_16k);

            // ★★★ **นี่คือข้อที่ P5-4 มีไว้ทำ**: RAM ต้องไม่ไต่ตามขนาดที่ผู้ใช้เลือก
            //     16384² มีพิกเซลมากกว่า 4096² ถึง 16 เท่า ถ้า RAM ไต่ตาม
            //     ส่วนต่างจะเป็นหลัก GB ไม่ใช่หลักสิบ MB
            assert!(
                at_16k <= at_4k + (32 << 20),
                "{label}: 16384² กิน {} MB ส่วน 4096² กิน {} MB — เพดานไต่ตามขนาด",
                at_16k >> 20,
                at_4k >> 20
            );
        }

        let ceiling = (PEAK_CEILING + (32 << 20)) as u64;
        println!(
            "ยอดสูงสุดตลอดการวัด: {} MB (เพดานของเทสต์ {} MB = {} MB ตามสเปก + 32 MB ให้ตัวเข้ารหัส)",
            worst >> 20,
            ceiling >> 20,
            PEAK_CEILING >> 20
        );
        assert!(
            worst <= ceiling,
            "export กิน RSS สูงสุด {} MB เกินเพดาน {} MB",
            worst >> 20,
            ceiling >> 20
        );
    }
}
