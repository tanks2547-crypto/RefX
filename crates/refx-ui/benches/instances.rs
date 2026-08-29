//! benchmark ของ `docs/08 §2` แถว `build_instances_1000` (เพดาน 300 µs)
//!
//! วัด [`refx_ui::instances::build_instances`] — งานที่เกิดขึ้น **ทุกครั้งที่
//! `Board` เปลี่ยน**: เดิน z-order ทั้งกระดาน · ถามชนิดจาก board · หาสถานะการวาด
//! · แปลงเรขาคณิตเป็น `QuadInstance`
//!
//! ★ ไม่ต้องมี GPU: ฟังก์ชันนี้ผลิต **ข้อมูล** ที่ GPU จะเอาไปวาดทีหลัง
//! การอัปโหลดขึ้น buffer เป็นคนละขั้นและอยู่ในแถว `frame_pan_1000`
//!
//! ## ★★ สิ่งที่ทำให้ตัวเลขนี้จริงหรือไม่จริง
//!
//! ถ้าให้ทุกใบ `slot = None` มันจะวิ่งเข้ากิ่ง placeholder ซึ่ง **ไม่เรียก
//! `crop_uv` เลย** — เร็วกว่าความจริงโดยไม่มีใครเห็น · กระดานจริงที่ผู้ใช้ดูอยู่
//! คือกระดานที่ภาพส่วนใหญ่อยู่บน atlas แล้ว จึงให้ 90% มีช่องจริง
//! และให้ crop/หมุน/ฟิลเตอร์ปนอยู่ตามสัดส่วนที่ผู้ใช้ใช้จริง
//!
//! spec: docs/08 §2, ROADMAP P5-1

// `criterion_group!` แผ่ `fn benches()` ออกมาโดยไม่มี doc comment ให้
#![allow(missing_docs)]

use criterion::{Criterion, criterion_group, criterion_main};
use glam::Vec2;
use refx_core::arena::{ArenaKey as _, BoardId, ItemId};
use refx_core::board::{
    AssetRef, Board, BoardParts, CropRect, Flip, ImageFormat, Item, ItemKind, ItemParts,
};
use refx_core::hash::ContentHash;
use refx_core::layout::{Engine, LayoutParams, layout};
use refx_render::atlas::AtlasSlot;
use refx_render::instance::QuadInstance;
use refx_ui::instances::build_instances;

const N: usize = 1000;

/// สัดส่วนภาพที่นักวาดมีจริง (ชุดเดียวกับ `refx-core/benches`)
const ASPECTS: [(f32, f32); 6] = [
    (3.0, 2.0),
    (2.0, 3.0),
    (16.0, 9.0),
    (1.0, 1.0),
    (210.0, 297.0),
    (5.0, 1.0),
];

/// กระดานที่ **วางด้วย layout engine ตัวจริง** แล้วมีสภาพผสมเหมือนของผู้ใช้
///
/// ★ 1 ใน 8 ใบถูกครอบ · 1 ใน 5 ถูกหมุน · 1 ใน 6 เปิดฟิลเตอร์ · 1 ใน 10 ยังไม่มี
/// ช่องใน atlas · 1 ใน 40 ถูกซ่อน — สัดส่วนพวกนี้คือสิ่งที่แยก "วัดของจริง"
/// ออกจาก "วัดกิ่งที่ถูกที่สุดพันครั้ง"
fn mood_board(n: usize) -> Board {
    let sizes: Vec<(ItemId, Vec2)> = (0..n)
        .map(|i| {
            let (w, h) = ASPECTS[i % ASPECTS.len()];
            let scale = 900.0 + (i % 7) as f32 * 260.0;
            let id = ItemId::from_parts(u32::try_from(i).unwrap_or(u32::MAX), 0);
            (id, Vec2::new(w * scale / h.max(1.0), scale))
        })
        .collect();
    let placed = layout(
        Engine::JustifiedRows,
        &sizes,
        LayoutParams {
            width: 12_000.0,
            gap: 8.0,
            target_row_height: 220.0,
            columns: None,
        },
    );

    let items: Vec<ItemParts> = placed
        .iter()
        .enumerate()
        .map(|(i, p)| {
            let mut item = Item::new(ItemKind::Image(AssetRef {
                hash: ContentHash::from_bytes([(i % 251) as u8; 32]),
                path: std::path::PathBuf::from("bench.png"),
                px_size: glam::UVec2::new(1920, 1080),
                format: ImageFormat::Unknown,
                embedded: false,
                mtime: 0,
                file_size: 0,
            }));
            item.canvas.pos = p.centre();
            item.canvas.size = p.size;
            if i % 8 == 0 {
                item.canvas.crop = CropRect {
                    min: Vec2::new(0.1, 0.05),
                    max: Vec2::new(0.9, 0.95),
                };
            }
            if i % 5 == 0 {
                item.canvas.rotation = 0.3;
            }
            if i % 6 == 0 {
                item.canvas.filter.grayscale = true;
            }
            if i % 9 == 0 {
                item.canvas.flip = Flip::Horizontal;
            }
            if i % 40 == 0 {
                item.canvas.visible = false; // ใบที่ `quad_for` ต้องคืน None
            }
            item.canvas.opacity = 0.5 + (i % 3) as f32 * 0.25;
            item.meta.added_at = 0;
            ItemParts { item, group: None }
        })
        .collect();

    Board::load(
        BoardId::from_parts(0, 0),
        BoardParts {
            name: "bench".to_owned(),
            items,
            ..BoardParts::default()
        },
    )
}

fn bench_build_instances(c: &mut Criterion) {
    let board = mood_board(N);
    // 1 ใน 10 ยังไม่มีช่องใน atlas → เดินกิ่ง placeholder ตามสัดส่วนจริง
    let state_of = |id: ItemId| {
        let index = id.index() as usize;
        let slot = (!index.is_multiple_of(10)).then_some(AtlasSlot {
            layer: (index / 256) as u32,
            index: (index % 256) as u32,
        });
        Some((slot, [0.4, 0.5, 0.6, 1.0]))
    };

    // ★ พิมพ์ให้ตรวจได้ว่าไม่ได้วัดกระดานว่าง หรือวัดแต่กิ่งเดียว
    let mut out = Vec::new();
    build_instances(&board, &mut out, state_of);
    println!(
        "build_instances_1000: สร้าง {} instance จาก {N} ใบ (ที่เหลือถูกซ่อนไว้)",
        out.len()
    );
    assert!(
        out.len() > N * 9 / 10 && out.len() < N,
        "สัดส่วนที่วาดจริงผิดไปจากที่ตั้งใจ ({} ใบ) — ตัวเลขจะไม่ใช่ของกระดานจริง",
        out.len()
    );

    c.bench_function("build_instances_1000", |b| {
        // ใช้บัฟเฟอร์เดิมซ้ำเหมือนเส้นทางจริง (`gfx.quads` ถูก clear แล้วเติมใหม่)
        let mut out: Vec<QuadInstance> = Vec::with_capacity(N);
        b.iter(|| {
            build_instances(std::hint::black_box(&board), &mut out, state_of);
            std::hint::black_box(out.len())
        });
    });
}

criterion_group!(benches, bench_build_instances);
criterion_main!(benches);
