//! ★★★ ราคาจริงของ export ในหน่วยความจำของโปรเซส (P5-4 · `docs/07 §6`)
//!
//! ## ทำไมอยู่ในไฟล์เทสต์ของตัวเอง ไม่ใช่ `mod tests` ข้าง ๆ โค้ด
//!
//! RSS เป็นของ **ทั้งโปรเซส** ไม่ใช่ของเทสต์ตัวใดตัวหนึ่ง · ตอนอยู่รวมกับเทสต์
//! อีก 221 ตัวใน binary เดียว มันวัดของคนอื่นติดมาด้วย และผลออกมาไม่คงที่:
//!
//! | รันแบบไหน | 4096² วัดได้ |
//! |---|---|
//! | ลำพัง | **+16 MB** |
//! | ปนกับเทสต์อื่นทั้งชุด | **+104 MB** |
//!
//! ตัวการที่ชัดที่สุดคือ **NC ของ tile จตุรัสเอง** ซึ่งจอง 64 MB — เทสต์สองตัว
//! ที่ใช้มาตรวัดเดียวกันจึงกัดกันเองเมื่อรันพร้อมกัน (เห็นจริง 8 ก.ย. 2026:
//! `cargo test --all` แดงเป็นครั้งคราวทั้งที่รันลำพังเขียวทุกครั้ง)
//!
//! → ย้ายมาเป็น **integration test ของตัวเอง** ซึ่ง cargo คอมไพล์เป็น binary
//! แยกและรันคนละโปรเซส · และรวมทุกขั้นไว้ใน `#[test]` **ตัวเดียว** เพื่อให้
//! มันเรียงกันแน่นอน ไม่ใช่หวังว่า scheduler จะไม่ให้มันซ้อนกัน
//!
//! ★ `docs/08 §3.9` ข้อ 16: เพดานต้องบังคับ ณ จุดที่ทุกชิ้นมีชีวิตอยู่พร้อมกัน
//!   — และข้อ 9: เครื่องมือที่ผลิตหลักฐานต้องพิสูจน์ก่อนว่าตัวมันเองไม่โกหก
//!
//! ## ★★★ เทสต์นี้รันที่ไหนบ้าง — และทำไมไม่รันบน runner ของ Windows
//!
//! มันเรนเดอร์ tile บน GPU แล้วอ่านกลับ · **นาฬิกาของมันคือนาฬิกาของ
//! rasterizer ไม่ใช่ของโค้ดเรา** วัดจริง 15 ก.ย. 2026:
//!
//! | ที่ไหน | adapter | เวลารวม |
//! |---|---|---|
//! | เครื่องพัฒนา | RTX 4060 (GPU จริง) | **19 วิ** |
//! | ubuntu runner | llvmpipe 256-bit | **109 วิ** |
//! | windows runner | **WARP** `Microsoft Basic Render Driver` | **> 600 วิ** |
//!
//! ★ ค่า RSS ที่เทสต์นี้มีไว้ตรวจ **ผ่านหมดทุกขั้นบน WARP ด้วย** — ที่ล้มคือ
//! นาฬิกาอย่างเดียว · `ci.yml` จึงตัดมันออกจากขา Windows พร้อมพิมพ์บอกใน log
//!
//! ★★ **นี่ไม่ใช่การเลิกตรวจบน Windows** — เครื่องพัฒนาทุกเครื่องเป็น Windows
//! และรันมันทุกครั้งที่ใครรันชุดเทสต์ · ที่ถูกตัดคือ runner ที่ไม่มี GPU จริง

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use refx_asset::export::ExportFormat;
use refx_core::export::PEAK_CEILING;
use refx_core::geom::Rect;
use refx_core::glam::Vec2;
use refx_render::atlas::ThumbnailAtlas;
use refx_render::instance::{QuadInstance, pack_tint};
use refx_render::pipeline::DrawBatch;
use refx_render::texture::TextureAllocator;
use refx_ui::export::{ExportRequest, GpuAccess, JobSignals, export_board};

/// GPU + atlas ที่มีพิกเซลจริงอยู่หนึ่งช่อง
fn scene() -> Option<(
    wgpu::Device,
    wgpu::Queue,
    TextureAllocator,
    ThumbnailAtlas,
    wgpu::Instance,
)> {
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
    // ★ ไม่มีการ์ดจริงก็ยังเทสต์ได้ — ขอ software adapter แทน
    let ask = |fallback| {
        block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::LowPower,
            compatible_surface: None,
            force_fallback_adapter: fallback,
        }))
        .and_then(Result::ok)
    };
    let adapter = ask(false).or_else(|| ask(true))?;
    let info = adapter.get_info();
    let limits = adapter.limits();
    let (device, queue) = block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        label: Some("refx-export-ram"),
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
fn block_on<F: std::future::Future>(future: F) -> Option<F::Output> {
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

fn temp_dir() -> PathBuf {
    let dir = std::env::temp_dir().join(format!("refx-export-ram-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn plain_rename(from: &std::path::Path, to: &std::path::Path) -> std::io::Result<()> {
    std::fs::rename(from, to)
}

/// ★★★ สามขั้นที่ต้องเรียงกัน — ดูหัวไฟล์ว่าทำไมไม่แยกเป็นสาม `#[test]`
///
/// 1. มาตรวัดขยับจริงไหม (ถ้าไม่ ตัวเลขข้างล่างก็เชื่อไม่ได้)
/// 2. ทาง tile **จตุรัส** ที่สเปกรุ่นแรกเขียนไว้ พังจริงไหม (NC)
/// 3. ทางที่เลือกจริง (tile **ทรงแถบ**) กิน RSS เท่าไหร่ที่แต่ละขนาด
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
    let dir = temp_dir();

    // ---------- 1. มาตรวัดต้องพิสูจน์ว่ามันขยับจริง (`§3.9` ข้อ 9) ----------
    {
        let before = refx_platform::memory::process_memory().unwrap().current;
        const BLOCK: usize = 64 << 20;
        let mut hog = vec![0u8; BLOCK];
        for page in hog.chunks_mut(4096) {
            page[0] = 1;
        }
        let growth = refx_platform::memory::process_memory()
            .unwrap()
            .current
            .saturating_sub(before);
        println!(
            "มาตรวัด: จอง {} MB แล้วแตะทุกหน้า → RSS +{} MB",
            BLOCK >> 20,
            growth >> 20
        );
        assert!(
            growth >= (BLOCK as u64) / 2,
            "มาตรวัดไม่ขยับ ({growth} ไบต์) — ตัวเลขทุกตัวข้างล่างเชื่อไม่ได้"
        );
        assert_eq!(hog[0], 1); // กัน optimizer ตัดบล็อกทิ้งก่อนถึงจุดวัด
    }

    // ---------- 2. NC: ทาง tile จตุรัสพังจริง ----------
    {
        let before = refx_platform::memory::process_memory().unwrap().current;
        let square = u64::from(refx_core::export::TILE_WIDTH)
            * u64::from(refx_core::export::TILE_WIDTH)
            * u64::from(refx_core::export::BYTES_PER_PIXEL);
        let buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("refx-nc-square-tile"),
            size: square,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: true,
        });
        // ★ เขียนลงไปจริงเพื่อให้มันเข้า working set (`BufferViewMut` เขียนได้อย่างเดียว)
        {
            let mut mapped = buffer.slice(..).get_mapped_range_mut();
            let page = vec![1u8; 1 << 20];
            for chunk in 0..(square / (1 << 20)) {
                let at = (chunk * (1 << 20)) as usize;
                mapped.slice(at..at + page.len()).copy_from_slice(&page);
            }
        }
        let growth = refx_platform::memory::process_memory()
            .unwrap()
            .current
            .saturating_sub(before);
        // ทางที่ไม่ได้เลือกต้องถือ **ทั้งสองอย่างพร้อมกัน**: บัฟเฟอร์อ่านกลับของ
        // tile จตุรัส และบัฟเฟอร์แถบที่ป้อนตัวเข้ารหัส
        let square_design = growth + refx_core::export::BAND_BUDGET as u64;
        println!(
            "NC วิ่งผ่านจริง: บัฟเฟอร์อ่านกลับของ tile จตุรัส 4096² = {square} ไบต์ \
             → RSS +{growth} ไบต์ · บวกบัฟเฟอร์แถบอีก {} MB = {} MB ซึ่งเกินเพดาน {} MB",
            refx_core::export::BAND_BUDGET >> 20,
            square_design >> 20,
            PEAK_CEILING >> 20
        );
        assert!(
            growth >= square * 3 / 4,
            "NC ไม่แดง — บัฟเฟอร์ {square} ไบต์ควรเข้า working set จริง แต่ RSS ขยับแค่ {growth}"
        );
        assert!(
            square_design > PEAK_CEILING as u64,
            "NC ไม่แดง — ทาง tile จตุรัสควรเกินเพดาน แต่รวมแล้วได้แค่ {} MB \
             ถ้าเป็นแบบนั้นจริง เหตุผลที่เลือกทรงแถบก็ไม่มีหลักฐานรองรับ",
            square_design >> 20
        );
        buffer.unmap();
    }

    // ---------- 3. ราคาจริงของทางที่เลือก ----------
    //
    // ★★ วัดด้วย **เธรดที่คอยอ่าน RSS ระหว่างทาง** ไม่ใช่ค่าก่อน–หลัง
    //    ยอดดอยเกิดกลางทางแล้วหายไปก่อนเราจะอ่านค่าหลังเสร็จ
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
        let peak = Arc::new(AtomicU64::new(0));
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
            JobSignals::just_cancel(&AtomicBool::new(false)),
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

    let ceiling = (PEAK_CEILING + (32 << 20)) as u64;
    for format in [
        ExportFormat::Png { transparent: false },
        ExportFormat::Jpeg { quality: 90 },
    ] {
        let label = format.name();
        let at_4k = run(4096, format, label);
        let at_8k = run(8192, format, label);
        let at_16k = run(16384, format, label);
        let smallest = at_4k.min(at_8k).min(at_16k);

        // ★★★ **นี่คือข้อที่ P5-4 มีไว้ทำ**: RAM ต้องไม่ไต่ตามขนาดที่ผู้ใช้เลือก
        //     16384² มีพิกเซลมากกว่า 4096² ถึง 16 เท่า ถ้า RAM ไต่ตาม
        //     ส่วนต่างจะเป็นหลัก GB ไม่ใช่หลักสิบ MB
        assert!(
            at_16k <= smallest + (32 << 20),
            "{label}: 16384² กิน {} MB ส่วนช่องที่น้อยสุดกิน {} MB — เพดานไต่ตามขนาด",
            at_16k >> 20,
            smallest >> 20
        );
        assert!(
            at_16k <= ceiling,
            "{label}: 16384² กิน RSS {} MB เกินเพดาน {} MB ({} MB ตามสเปก + 32 MB ให้ตัวเข้ารหัส)",
            at_16k >> 20,
            ceiling >> 20,
            PEAK_CEILING >> 20
        );
    }
    let _ = std::fs::remove_dir_all(&dir);
}
