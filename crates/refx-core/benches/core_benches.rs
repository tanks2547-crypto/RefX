//! benchmark ของ `refx-core` — สองแถวใน `docs/08 §2` ที่วัดได้โดยไม่ต้องมี GPU
//!
//! | bench | เพดาน | วัดอะไร |
//! |---|---|---|
//! | `cull_1000` | 200 µs | `SpatialIndex::query_into` — culling ที่รันทุกเฟรม |
//! | `layout_justified_1000` | 2 ms | `layout(Engine::JustifiedRows, …)` ที่รันตอนกด Apply |
//!
//! ## ★★★ ข้อมูลที่ใช้วัดต้องสมจริง ไม่งั้นตัวเลขที่ได้ไม่มีความหมาย
//!
//! โปรเจกต์นี้พลาดรูปแบบนี้มาแล้วสามครั้ง (วัด snapshot ด้วยโน้ตข้อความ · วัด
//! การเขียน PNG ด้วยภาพ gradient · เลข 1.07s ที่เป็นของ test profile) — ทุกครั้ง
//! ตัวเลขที่ได้ *ดูดี* และ *ไม่จริง*
//!
//! ที่นี่จึงสร้าง board ด้วย **layout engine ตัวจริงของโปรแกรม** แล้ววาง item
//! ตามผลของมัน · กระดานที่ได้จึงมีรูปร่างเหมือนสิ่งที่ผู้ใช้เห็นจริง ไม่ใช่ตาราง
//! สม่ำเสมอที่ทำให้ grid ของ `SpatialIndex` ทำงานง่ายเกินจริง
//!
//! ★★ สำหรับ `cull_1000` **การกระจายตัวสำคัญกว่าจำนวน**: ถ้าวาง item ทั้งพันใบ
//! ไว้มุมเดียวแล้ว query อีกมุมหนึ่ง culling จะเร็วจนไร้ความหมาย · กรอบที่ใช้ค้น
//! จึงเป็นหน้าต่าง 1920×1080 ที่วางกลางกระดาน ซึ่งเป็นสิ่งที่ผู้ใช้เห็นจริง
//! และเทสต์พิมพ์จำนวนที่มองเห็นออกมาด้วยเพื่อให้ตรวจได้ว่าไม่ได้วัดกรอบว่าง
//!
//! spec: docs/08 §2, ROADMAP P5-1

// `criterion_group!` แผ่ `fn benches()` ออกมาโดยไม่มี doc comment ให้ —
// `missing_docs` ของ workspace จึงล้มที่ macro ซึ่งเราแก้ที่ต้นทางไม่ได้
#![allow(missing_docs)]

use criterion::{Criterion, criterion_group, criterion_main};
use glam::Vec2;
use refx_core::arena::{ArenaKey as _, BoardId, ItemId};
use refx_core::board::{
    AssetRef, Board, BoardParts, ImageFormat, Item, ItemKind, ItemParts, TextNote,
};
use refx_core::geom::Rect;
use refx_core::hash::ContentHash;
use refx_core::layout::{Engine, LayoutParams, layout};
use refx_core::spatial::SpatialIndex;

/// จำนวนภาพที่ทุก bench ในไฟล์นี้ใช้ — ตรงกับชื่อแถวใน `docs/08 §2`
const N: usize = 1000;

/// ★ สัดส่วนภาพที่นักวาดมีจริง — ไม่ใช่จัตุรัสล้วนซึ่งทำให้ layout ง่ายเกินจริง
///
/// ภาพถ่าย 3:2 ทั้งสองแนว · หน้าจอ 16:9 · สแกนงาน A4 · พาโนรามาที่กว้างมาก
/// (ตัวสุดท้ายสำคัญ เพราะมันคือใบที่ทำให้แถวของ `JustifiedRows` ต้องปรับตัวแรงสุด)
const ASPECTS: [(f32, f32); 6] = [
    (3.0, 2.0),
    (2.0, 3.0),
    (16.0, 9.0),
    (1.0, 1.0),
    (210.0, 297.0),
    (5.0, 1.0),
];

/// รายการ `(id, ขนาดต้นฉบับ)` ที่ layout engine รับเข้าไป
fn items(n: usize) -> Vec<(ItemId, Vec2)> {
    (0..n)
        .map(|i| {
            let (w, h) = ASPECTS[i % ASPECTS.len()];
            // ขนาดต้นฉบับต่างกันจริง ไม่ใช่สเกลเดียวกันทั้งกระดาน
            let scale = 900.0 + (i % 7) as f32 * 260.0;
            let id = ItemId::from_parts(u32::try_from(i).unwrap_or(u32::MAX), 0);
            (id, Vec2::new(w * scale / h.max(1.0), scale))
        })
        .collect()
}

/// พารามิเตอร์ที่ตรงกับหน้าต่างจริง — กว้าง 1600 คือแผง Arrange บนจอ 1920
fn params() -> LayoutParams {
    LayoutParams {
        width: 1600.0,
        gap: 8.0,
        target_row_height: 220.0,
        columns: None,
    }
}

/// ★★ พารามิเตอร์สำหรับ **กระดาน Canvas** ซึ่งเป็นคนละรูปร่างกับแผง Arrange
///
/// `params()` (กว้าง 1600) ให้กระดานสูง ~50,000 หน่วยแต่กว้างแค่ 1600 — ซึ่งถูก
/// สำหรับ Arrange (แผงที่เลื่อนลงอย่างเดียว) แต่ **เป็นรูปร่างที่ผิดสำหรับการวัด
/// culling**: grid ของ `SpatialIndex` จะมีช่องเรียงเป็นแถวเดียวยาว ๆ ซึ่งค้นง่าย
/// กว่าความจริงมาก
///
/// mood board จริงกระจายสองมิติ — กว้าง 12,000 ให้กระดานราว 12,000 × 6,400
/// ซึ่งเป็นสัดส่วนที่ผู้ใช้จัดจริงเวลาวางภาพอ้างอิงเทียบกัน
fn canvas_params() -> LayoutParams {
    LayoutParams {
        width: 12_000.0,
        ..params()
    }
}

/// ★ board ที่ item ถูกวาง **ด้วย layout engine ตัวจริง** แล้วบวก jitter เล็กน้อย
///
/// jitter จำลองการที่ผู้ใช้ลากภาพเองหลังกด Apply — ทำให้ item ไม่ตกลงช่องของ
/// `SpatialIndex` อย่างเป็นระเบียบเกินจริง
fn mood_board(n: usize) -> Board {
    let placed = layout(Engine::JustifiedRows, &items(n), canvas_params());
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
            let jitter = Vec2::new(((i % 13) as f32 - 6.0) * 3.0, ((i % 11) as f32 - 5.0) * 3.0);
            item.canvas.pos = p.centre() + jitter;
            item.canvas.size = p.size;
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

/// หน้าต่าง 1920×1080 ที่วางกลางกระดาน — สิ่งที่ผู้ใช้เห็นจริงที่ zoom = 1
fn viewport(board: &Board) -> Rect {
    let mut bounds = Rect::EMPTY;
    for (_, item) in board.items_in_z_order() {
        bounds = bounds.union(item.canvas.obb().aabb());
    }
    Rect::from_center_size(bounds.center(), Vec2::new(1920.0, 1080.0))
}

fn bench_cull(c: &mut Criterion) {
    let board = mood_board(N);
    let index = SpatialIndex::from_board(&board);
    let rect = viewport(&board);

    // ★ พิมพ์ออกมาให้ตรวจได้ว่าไม่ได้วัดกรอบว่าง — ถ้า visible เป็น 0
    //   ตัวเลขที่ได้จะสวยมากและไม่มีความหมายเลย
    let mut out = Vec::new();
    let stats = index.query_into(rect, &mut out);
    println!(
        "cull_1000: เห็น {} จาก {N} ใบ · เปิดดู {} ช่อง · ทดสอบจริง {} ใบ",
        out.len(),
        stats.cells_visited,
        stats.items_examined
    );
    assert!(!out.is_empty(), "กรอบที่ใช้วัดไม่เห็นอะไรเลย — ตัวเลขจะไร้ความหมาย");
    assert!(out.len() < N, "กรอบที่ใช้วัดเห็นทั้งกระดาน — ไม่ได้วัด culling");

    c.bench_function("cull_1000", |b| {
        // ใช้บัฟเฟอร์เดิมซ้ำเหมือนที่เส้นทางจริงทำ (culling ทุกเฟรม)
        let mut out = Vec::with_capacity(256);
        b.iter(|| {
            let stats = index.query_into(std::hint::black_box(rect), &mut out);
            std::hint::black_box((out.len(), stats.items_examined))
        });
    });
}

fn bench_layout(c: &mut Criterion) {
    let items = items(N);
    let params = params();
    c.bench_function("layout_justified_1000", |b| {
        b.iter(|| {
            std::hint::black_box(layout(
                Engine::JustifiedRows,
                std::hint::black_box(&items),
                std::hint::black_box(params),
            ))
        });
    });
}

criterion_group!(benches, bench_cull, bench_layout);
criterion_main!(benches);

// ★ `TextNote` ถูก import ไว้เพื่อให้ชัดว่า board ของ bench เป็นภาพล้วนโดยตั้งใจ
//   (โน้ตข้อความคือสิ่งที่ทำให้การวัด snapshot เพี้ยนมาแล้วครั้งหนึ่ง)
#[allow(dead_code)]
fn _text_note_is_deliberately_absent(_: TextNote) {}
