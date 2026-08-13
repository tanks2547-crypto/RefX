//! Sort + filter ของ Arrange mode (P3-4) — **ฟังก์ชันบริสุทธิ์ทั้งไฟล์**
//!
//! รับ `&Board` คืน **ลำดับของ `ItemId`** ไม่แตะ board และไม่ถือสถานะอะไรเลย
//! ส่วนการ *ไม่คำนวณซ้ำ* เป็นหน้าที่ของผู้เรียก ซึ่งเทียบ [`Board::revision`]
//! (docs/03 §3 — ที่ 1000+ ภาพ การกรองใหม่ทุกเฟรมคือการเผา CPU ฟรี)
//!
//! ★★ **ต้อง deterministic** เหมือน `layout` — ลำดับที่สลับกันเองระหว่างเฟรม
//! ผู้ใช้เห็นเป็นภาพกระโดดสลับที่ · ทุกตัวเปรียบเทียบจึงจบด้วย **ดัชนีใน z-order**
//! เป็นตัวตัดสินท้ายเสมอ ไม่มีที่ไหนปล่อยให้ค่าที่เท่ากันตัดสินกันเอง
//!
//! ★ **ทำไมไม่รับ `Vec<Row>` ที่ผู้เรียกประกอบมา** — ข้อมูลทุกอย่างที่ใช้เรียง/กรอง
//! อยู่ใน `Board` แล้ว การให้ผู้เรียกก๊อปออกมาเป็นโครงคู่ขนานคือแหล่งความจริงที่สอง
//! ซึ่งเป็นปัญหาเดียวกับที่ §2.2 ใช้เวลาทั้ง session แก้ (quads คู่ขนานกับ board)
//!
//! spec: docs/03-modes-and-ui.md §3, ROADMAP P3-4

use crate::arena::ItemId;
use crate::board::{Board, ColorLabel, ItemKind, SortKey, TagId};
use glam::Vec2;

/// เลือกป้ายสีแบบไหน
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LabelFilter {
    /// ไม่กรองด้วยป้ายสี
    #[default]
    Any,
    /// เฉพาะใบที่ **ไม่มี** ป้าย — ★ `Option<ColorLabel>` เดี่ยว ๆ พูดข้อนี้ไม่ได้
    /// (`None` ในนั้นแปลว่า "ไม่กรอง" ไปแล้ว) จึงต้องเป็น enum ของตัวเอง
    Unlabelled,
    /// เฉพาะป้ายสีนี้
    Is(ColorLabel),
}

/// เงื่อนไขของแท็ก
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TagMode {
    /// มีอย่างน้อยหนึ่งแท็กในรายการ
    #[default]
    Any,
    /// ต้องมีครบทุกแท็กในรายการ
    All,
    /// ต้องไม่มีแท็กไหนในรายการเลย
    None,
}

/// ตัวกรองของ Arrange mode
///
/// ★ ค่าปริยาย = **ไม่กรองอะไรเลย** ([`Filter::is_open`]) — ตัวกรองที่เปิดโปรแกรม
/// มาแล้วซ่อนภาพของผู้ใช้อยู่คือสิ่งที่แย่ที่สุดที่ตัวกรองทำได้
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Filter {
    /// ดาวขั้นต่ำ — `0` = ไม่กรอง
    pub min_rating: u8,
    /// ป้ายสี
    pub label: LabelFilter,
    /// แท็กที่สนใจ (ว่าง = ไม่กรอง)
    pub tags: smallvec::SmallVec<[TagId; 4]>,
    /// ตีความรายการแท็กยังไง
    pub tag_mode: TagMode,
    /// คำค้น — หาใน **ชื่อไฟล์ · โน้ตของ item · ข้อความของโน้ต** (ว่าง = ไม่กรอง)
    pub text: String,
    /// เฉพาะใบที่ปักหมุด
    pub pinned_only: bool,
}

impl Filter {
    /// ตัวกรองนี้ปล่อยผ่านทุกใบหรือไม่
    ///
    /// ★ ผู้เรียกใช้ตัดสินว่าจะบอกผู้ใช้ไหมว่า "กำลังกรองอยู่" — ผู้ใช้ที่มองหา
    /// ภาพที่หายไปต้องเห็นได้ทันทีว่ามันถูกกรองอยู่ ไม่ใช่คิดว่ามันหาย
    #[must_use]
    pub fn is_open(&self) -> bool {
        self.min_rating == 0
            && self.label == LabelFilter::Any
            && self.tags.is_empty()
            && self.text.is_empty()
            && !self.pinned_only
    }
}

/// item ใบนี้ผ่านตัวกรองไหม
///
/// `id` ที่ไม่มีอยู่บน board = ไม่ผ่าน (ไม่ใช่ panic — id ตายได้เสมอหลัง undo)
#[must_use]
pub fn matches(board: &Board, id: ItemId, filter: &Filter) -> bool {
    let Some(item) = board.item(id) else {
        return false;
    };
    if item.meta.rating < filter.min_rating {
        return false;
    }
    match filter.label {
        LabelFilter::Any => {}
        LabelFilter::Unlabelled => {
            if item.meta.color_label.is_some() {
                return false;
            }
        }
        LabelFilter::Is(want) => {
            if item.meta.color_label != Some(want) {
                return false;
            }
        }
    }
    if filter.pinned_only && !item.meta.pinned {
        return false;
    }
    if !filter.tags.is_empty() {
        let has = |tag: &TagId| item.meta.tags.contains(tag);
        let ok = match filter.tag_mode {
            TagMode::Any => filter.tags.iter().any(has),
            TagMode::All => filter.tags.iter().all(has),
            TagMode::None => !filter.tags.iter().any(has),
        };
        if !ok {
            return false;
        }
    }
    if !filter.text.is_empty() {
        // ★ เทียบแบบไม่สนตัวพิมพ์ · ผู้ใช้พิมพ์ "SKY" แล้วต้องเจอ "sky.png"
        //   `to_lowercase` (ไม่ใช่ `to_ascii_lowercase`) เพราะชื่อไฟล์เป็นภาษาอะไรก็ได้
        let needle = filter.text.to_lowercase();
        if !haystack(item).to_lowercase().contains(&needle) {
            return false;
        }
    }
    true
}

/// ข้อความทั้งหมดของ item ที่คำค้นควรหาเจอ
fn haystack(item: &crate::board::Item) -> String {
    let mut text = match &item.kind {
        // ★ ชื่อไฟล์อย่างเดียว **ไม่ใช่ path เต็ม** — ผู้ใช้ค้นคำว่า "ref" ไม่ได้
        //   ตั้งใจให้เจอทุกภาพเพียงเพราะมันอยู่ในโฟลเดอร์ชื่อ `references/`
        ItemKind::Image(asset) => asset
            .path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default(),
        ItemKind::Text(note) => note.text.clone(),
        ItemKind::Missing { original_path, .. } => original_path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default(),
    };
    if !item.meta.note.is_empty() {
        text.push('\n');
        text.push_str(&item.meta.note);
    }
    text
}

/// กรองแล้วเรียง — ผลลัพธ์คือลำดับที่ Arrange จะวาง
///
/// ★★ **ผลต้องเหมือนเดิมเป๊ะทุกครั้งที่ input เท่าเดิม** รวมถึงตอนค่าที่ใช้เรียง
/// เท่ากัน (ซึ่งเป็นเรื่องปกติมาก: ดาว 0 เท่ากันทั้ง board) — ตัวตัดสินท้ายคือ
/// ลำดับใน z-order ซึ่งก็คือลำดับที่ผู้ใช้เพิ่มภาพเข้ามา
#[must_use]
pub fn select(board: &Board, filter: &Filter, sort: SortKey, descending: bool) -> Vec<ItemId> {
    let mut rows: Vec<(usize, ItemId)> = board
        .z_order()
        .iter()
        .enumerate()
        .filter(|(_, id)| matches(board, **id, filter))
        .map(|(index, id)| (index, *id))
        .collect();

    // ★ เรียงตามตำแหน่งบน canvas ไม่ใช่การเทียบรายคู่ — แยกเส้นทาง (P3-6)
    if sort == SortKey::CanvasOrder {
        canvas_reading_order(board, &mut rows, descending);
        let mut ordered: Vec<ItemId> = rows.into_iter().map(|(_, id)| id).collect();
        fold_collapsed_groups(board, &mut ordered);
        return ordered;
    }

    rows.sort_by(|(a_index, a), (b_index, b)| {
        compare(board, *a, *b, sort)
            // ★ ตัวตัดสินท้าย **ต้องไม่กลับด้านตาม `descending`** — ไม่งั้นสอง item
            //   ที่เท่ากันทุกอย่างจะสลับที่กันเองทุกครั้งที่ผู้ใช้กดสลับทิศ
            .then(a_index.cmp(b_index))
    });
    if descending {
        // กลับเฉพาะผลของ **คีย์** ไม่ใช่กลับทั้ง vec (ตัวที่เสมอกันต้องอยู่ลำดับเดิม)
        rows.sort_by(|(a_index, a), (b_index, b)| {
            compare(board, *a, *b, sort)
                .reverse()
                .then(a_index.cmp(b_index))
        });
    }
    let mut ordered: Vec<ItemId> = rows.into_iter().map(|(_, id)| id).collect();
    fold_collapsed_groups(board, &mut ordered);
    ordered
}

/// ★ กลุ่มที่ยุบอยู่เหลือสมาชิกโผล่ในแผ่นแค่ **ใบเดียว** (P3-7)
///
/// ★★ ตัวแทนคือ **ใบแรกที่เจอในลำดับที่เรียงเสร็จแล้ว** ไม่ใช่ใบแรกใน z-order
/// — ผู้ใช้ที่เรียงตามดาวแล้วยุบกลุ่ม คาดหวังว่าจะเห็นใบที่ดาวสูงสุดของกลุ่ม
/// เป็นหน้ากลุ่ม ไม่ใช่ใบที่บังเอิญถูกลากเข้ามาก่อน · เรียกหลังเรียงเสร็จเสมอ
/// ทั้งสองเส้นทาง จึง deterministic ตามลำดับที่เรียงมาแล้ว
///
/// ★ ทำที่นี่ ไม่ใช่ใน [`matches`] เพราะการยุบ **ไม่ใช่การกรอง**: ตัวกรอง
/// ตัดสินทีละใบโดยไม่ต้องรู้จักใบอื่น แต่ "ใบไหนได้เป็นตัวแทน" ต้องรู้ทั้งชุด
/// (ปัญหารูปเดียวกับ `canvas_reading_order` ของ P3-6)
///
/// > กลุ่มที่ยุบอยู่แต่ตัวแทนถูกตัวกรองตัดทิ้งไปแล้ว จะ **ไม่โผล่เลย** ซึ่งถูก:
/// > ตัวกรองเป็นคำสั่งของผู้ใช้ที่ตรงกว่า และการฝืนดึงใบที่ถูกกรองออกกลับมา
/// > จะทำให้ตัวกรองโกหก
fn fold_collapsed_groups(board: &Board, ordered: &mut Vec<ItemId>) {
    // ทางลัดที่ทำให้ราคาของฟีเจอร์นี้เป็นศูนย์ตราบใดที่ยังไม่มีใครยุบกลุ่ม
    if !board.groups().values().any(|group| group.collapsed) {
        return;
    }
    let mut shown: smallvec::SmallVec<[crate::arena::GroupId; 4]> = smallvec::SmallVec::new();
    ordered.retain(|id| {
        let Some(item) = board.item(*id) else {
            return false;
        };
        let Some(group_id) = item.meta.group else {
            return true;
        };
        // id ที่ห้อยอยู่ (กลุ่มถูกลบไปแล้ว) ถือว่าไม่ได้ยุบ — ห้ามซ่อนภาพของผู้ใช้
        // เพราะข้อมูลไม่ครบ (I-3)
        if !board.group(group_id).is_some_and(|group| group.collapsed) {
            return true;
        }
        if shown.contains(&group_id) {
            return false;
        }
        shown.push(group_id);
        true
    });
}

/// สัดส่วนของความสูงเฉลี่ยที่ยังถือว่า "อยู่แถวเดียวกัน" (docs/03 §4.2)
///
/// ★★ **ค่านี้คือทั้งหมดของ "เรียงแบบอ่านหนังสือ"** — mood board ไม่มีใครวางภาพ
/// ให้ขอบบนตรงกันเป๊ะ การเทียบ `y` ตรง ๆ จะได้ลำดับที่กระโดดไปมาระหว่างสองภาพ
/// ที่ตาเห็นว่าอยู่แถวเดียวกันชัด ๆ เพราะมันต่างกันสองพิกเซล
pub const ROW_TOLERANCE: f32 = 0.5;

/// เรียงแบบ "อ่านหนังสือ" จากตำแหน่งบน canvas — บน→ล่าง ซ้าย→ขวา (P3-6)
///
/// ★ **ไม่ใช่ตัวเปรียบเทียบรายคู่** จึงอยู่นอก [`compare`]: การตัดสินว่าสองใบ
/// อยู่แถวเดียวกันไหมต้องรู้ความสูงเฉลี่ยของ *ทั้งชุด* ก่อน — ฟังก์ชันเปรียบเทียบ
/// ที่เห็นทีละสองใบเขียนกฎนี้ไม่ได้เลย (และถ้าฝืนเขียน ผลจะไม่ transitive
/// ซึ่งทำให้ `sort_by` ให้ผลที่ไม่มีความหมาย)
fn canvas_reading_order(board: &Board, rows: &mut Vec<(usize, ItemId)>, descending: bool) {
    // 1. กรอบจริงของแต่ละใบ (AABB ของ OBB — ภาพที่หมุนใช้กรอบที่คลุมมันจริง)
    let bounds: Vec<(usize, ItemId, Vec2, f32)> = rows
        .iter()
        .filter_map(|(index, id)| {
            let item = board.item(*id)?;
            let rect = item.canvas.world_bounds();
            Some((*index, *id, rect.center(), rect.max.y - rect.min.y))
        })
        .collect();
    if bounds.is_empty() {
        return;
    }

    // 2. ความสูงเฉลี่ยของชุดนี้ → ระยะที่ยังนับว่าแถวเดียวกัน
    #[expect(clippy::cast_precision_loss, reason = "จำนวนภาพจริงอยู่ในช่วงที่ f32 แทนได้")]
    let average = bounds.iter().map(|(_, _, _, h)| *h).sum::<f32>() / bounds.len() as f32;
    // ★ ความสูงเป็น 0 หรือ NaN ได้ (ไฟล์เสีย/undo กลางคัน) — ระยะ 0 แปลว่า
    //   "ทุกใบคนละแถว" ซึ่งยังให้ผลที่เรียงตาม y ได้ ไม่ใช่ผลที่พัง (I-4)
    let tolerance = if average.is_finite() && average > 0.0 {
        average * ROW_TOLERANCE
    } else {
        0.0
    };

    // 3. เรียงตาม y ก่อน (เสมอกันตัดสินด้วย x แล้วด้วยดัชนี — deterministic)
    let mut by_y = bounds;
    by_y.sort_by(|a, b| {
        a.2.y
            .total_cmp(&b.2.y)
            .then(a.2.x.total_cmp(&b.2.x))
            .then(a.0.cmp(&b.0))
    });

    // 4. ไล่จากบนลงล่าง เปิดแถวใหม่เมื่อห่างจาก **ขอบบนของแถวปัจจุบัน** เกินระยะ
    //
    //    ★ ใช้ y ของใบแรกในแถวเป็นหมุด ไม่ใช่ค่าเฉลี่ยที่ขยับไปเรื่อย ๆ —
    //      ค่าเฉลี่ยที่เลื่อนตามทำให้ภาพที่วางไล่ระดับลงมาทีละนิดกลายเป็นแถวเดียว
    //      ยาวเหยียด ทั้งที่ใบแรกกับใบสุดท้ายห่างกันครึ่งจอ
    let mut row_of: Vec<(ItemId, u32, f32, usize)> = Vec::with_capacity(by_y.len());
    let mut row = 0u32;
    let mut anchor = by_y[0].2.y;
    for (index, id, centre, _) in by_y {
        if centre.y - anchor > tolerance {
            row += 1;
            anchor = centre.y;
        }
        row_of.push((id, row, centre.x, index));
    }

    // 5. ลำดับสุดท้าย: แถว → ซ้ายไปขวา → ดัชนีเดิม
    row_of.sort_by(|a, b| {
        let order = a.1.cmp(&b.1).then(a.2.total_cmp(&b.2));
        // ★ ตัวตัดสินท้าย **ไม่กลับด้าน** ตาม `descending` ด้วยเหตุผลเดียวกับ
        //   `select`: ของที่ทับกันสนิทต้องไม่สลับที่กันเองเวลากดสลับทิศ
        if descending { order.reverse() } else { order }.then(a.3.cmp(&b.3))
    });

    *rows = row_of
        .into_iter()
        .map(|(id, _, _, index)| (index, id))
        .collect();
}

/// เทียบสองใบตามคีย์ที่เลือก — **ไม่รวมตัวตัดสินท้าย** (ผู้เรียกใส่เอง)
fn compare(board: &Board, a: ItemId, b: ItemId, sort: SortKey) -> std::cmp::Ordering {
    use std::cmp::Ordering;
    let (Some(left), Some(right)) = (board.item(a), board.item(b)) else {
        return Ordering::Equal;
    };
    match sort {
        SortKey::AddedAt => left.meta.added_at.cmp(&right.meta.added_at),
        SortKey::Name => sort_name(left).cmp(&sort_name(right)),
        SortKey::Rating => left.meta.rating.cmp(&right.meta.rating),
        // ★ "ไม่มีป้าย" มาก่อนเสมอ แล้วเรียงตามค่าบนสาย (แดง…ม่วง แล้วค่อยป้าย
        //   ที่รุ่นนี้ไม่รู้จัก) — ใช้ `to_wire` เพราะมันคือ **สัญญาถาวร** (§2.11)
        //   ไม่ใช่ลำดับของ variant ที่ใครสลับได้
        SortKey::ColorLabel => label_order(left).cmp(&label_order(right)),
        SortKey::AspectRatio => aspect(left).total_cmp(&aspect(right)),
        // ★ ของที่ไม่ใช่ภาพ (โน้ต) ไม่มีไฟล์ → ค่า 0 มาก่อนเสมอ
        SortKey::ModifiedAt => source_mtime(left).cmp(&source_mtime(right)),
        SortKey::FileSize => source_bytes(left).cmp(&source_bytes(right)),
        // ★ ไม่มีทางมาถึงที่นี่ — `select` แยกเส้นทางไปแล้ว · เขียน `Equal`
        //   ไว้เพื่อให้ `match` ครบโดยไม่ต้องมี `_` (variant ใหม่ต้องมาคิดที่นี่)
        SortKey::CanvasOrder => Ordering::Equal,
    }
}

/// ชื่อที่ใช้เรียง — ★ พับตัวพิมพ์ก่อนเสมอ ไม่งั้น `Zebra.png` มาก่อน `apple.png`
/// เพราะ `Z` (0x5A) น้อยกว่า `a` (0x61) ซึ่งไม่ใช่สิ่งที่คนเรียกว่า "เรียงตามชื่อ"
fn sort_name(item: &crate::board::Item) -> String {
    match &item.kind {
        ItemKind::Image(asset) => asset
            .path
            .file_name()
            .map(|name| name.to_string_lossy().to_lowercase())
            .unwrap_or_default(),
        ItemKind::Text(note) => note.text.to_lowercase(),
        ItemKind::Missing { original_path, .. } => original_path
            .file_name()
            .map(|name| name.to_string_lossy().to_lowercase())
            .unwrap_or_default(),
    }
}

/// เวลาที่ไฟล์ต้นฉบับถูกแก้ (unix millis) — `0` เมื่อไม่มีไฟล์หรืออ่านไม่ได้
fn source_mtime(item: &crate::board::Item) -> i64 {
    match &item.kind {
        ItemKind::Image(asset) => asset.mtime,
        _ => 0,
    }
}

/// ขนาดไฟล์ต้นฉบับ (ไบต์) — `0` เมื่อไม่มีไฟล์
fn source_bytes(item: &crate::board::Item) -> u64 {
    match &item.kind {
        ItemKind::Image(asset) => asset.file_size,
        _ => 0,
    }
}

/// ลำดับของป้ายสี — `0` = ไม่มีป้าย ที่เหลือคือค่าบนสาย
fn label_order(item: &crate::board::Item) -> u8 {
    item.meta.color_label.map_or(0, ColorLabel::to_wire)
}

/// สัดส่วน กว้าง/สูง ของภาพต้นฉบับ — ของที่ไม่ใช่ภาพถือเป็นจัตุรัส
fn aspect(item: &crate::board::Item) -> f32 {
    match &item.kind {
        ItemKind::Image(asset) if asset.px_size.y > 0 => {
            #[expect(clippy::cast_precision_loss, reason = "ขนาดภาพจริงอยู่ในช่วงที่ f32 แทนได้")]
            let ratio = asset.px_size.x as f32 / asset.px_size.y as f32;
            if ratio.is_finite() { ratio } else { 1.0 }
        }
        _ => 1.0,
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use super::*;
    use crate::arena::ArenaKey as _;
    use crate::board::tests::image_item;
    use crate::board::{AssetRef, ImageFormat, Item, ItemKind, ItemMeta, TextNote};
    use crate::hash::ContentHash;
    use glam::UVec2;
    use std::path::PathBuf;

    fn image_named(name: &str, w: u32, h: u32) -> Item {
        Item::new(ItemKind::Image(AssetRef {
            hash: ContentHash::from_bytes([0; 32]),
            path: PathBuf::from(name),
            px_size: UVec2::new(w, h),
            format: ImageFormat::Png,
            embedded: false,
            mtime: 0,
            file_size: 0,
        }))
    }

    fn board_with(items: Vec<Item>) -> (Board, Vec<ItemId>) {
        let mut board = Board::default();
        let ids = items
            .into_iter()
            .map(|item| board.insert_item(item))
            .collect();
        (board, ids)
    }

    fn names(board: &Board, ids: &[ItemId]) -> Vec<String> {
        ids.iter()
            .map(|id| sort_name(board.item(*id).unwrap()))
            .collect()
    }

    #[test]
    fn sorting_by_name_folds_case_instead_of_comparing_bytes() {
        let (board, _) = board_with(vec![
            image_named("Zebra.png", 10, 10),
            image_named("apple.png", 10, 10),
            image_named("Mango.png", 10, 10),
        ]);
        let got = names(
            &board,
            &select(&board, &Filter::default(), SortKey::Name, false),
        );
        assert_eq!(got, vec!["apple.png", "mango.png", "zebra.png"]);
    }

    #[test]
    fn sorting_by_rating_and_flipping_the_direction() {
        let mut board = Board::default();
        let mut ids = Vec::new();
        for stars in [3u8, 0, 5, 1] {
            let id = board.insert_item(image_item(stars));
            board
                .set_meta(
                    id,
                    ItemMeta {
                        rating: stars,
                        ..ItemMeta::default()
                    },
                )
                .unwrap();
            ids.push(id);
        }
        let stars_of = |order: &[ItemId]| -> Vec<u8> {
            order
                .iter()
                .map(|id| board.item(*id).unwrap().meta.rating)
                .collect()
        };
        let up = select(&board, &Filter::default(), SortKey::Rating, false);
        assert_eq!(stars_of(&up), vec![0, 1, 3, 5]);
        let down = select(&board, &Filter::default(), SortKey::Rating, true);
        assert_eq!(stars_of(&down), vec![5, 3, 1, 0]);
    }

    /// ★★ ค่าที่เท่ากันต้องอยู่ลำดับเดิมเสมอ — และ **ไม่กลับด้านตาม `descending`**
    ///
    /// ที่ board จริงดาวเป็น 0 เท่ากันหมดทั้งกระดาน ถ้าตัวตัดสินท้ายกลับด้านตามทิศ
    /// ผู้ใช้กดสลับทิศแล้วภาพทั้งกระดานจะสลับที่ ทั้งที่ค่าที่เรียงเท่ากันหมด
    #[test]
    fn items_that_tie_keep_the_order_they_were_added_in_both_directions() {
        let (board, ids) = board_with((0..6).map(image_item).collect());
        let up = select(&board, &Filter::default(), SortKey::Rating, false);
        let down = select(&board, &Filter::default(), SortKey::Rating, true);
        assert_eq!(up, ids, "ทุกใบดาวเท่ากัน ลำดับต้องเป็นลำดับที่เพิ่มเข้ามา");
        assert_eq!(down, ids, "กดสลับทิศแล้วของที่เสมอกันสลับที่ตามไปด้วย");
    }

    #[test]
    fn sorting_by_aspect_puts_the_tallest_first() {
        let (board, _) = board_with(vec![
            image_named("wide.png", 40, 10),
            image_named("tall.png", 10, 40),
            image_named("square.png", 10, 10),
        ]);
        let got = names(
            &board,
            &select(&board, &Filter::default(), SortKey::AspectRatio, false),
        );
        assert_eq!(got, vec!["tall.png", "square.png", "wide.png"]);
    }

    #[test]
    fn sorting_by_colour_label_puts_unlabelled_first_and_follows_the_wire_order() {
        let mut board = Board::default();
        for label in [
            Some(ColorLabel::Blue),
            None,
            Some(ColorLabel::Red),
            ColorLabel::from_wire(200), // ป้ายจากรุ่นใหม่กว่า
        ] {
            let id = board.insert_item(image_item(1));
            board
                .set_meta(
                    id,
                    ItemMeta {
                        color_label: label,
                        ..ItemMeta::default()
                    },
                )
                .unwrap();
        }
        let order = select(&board, &Filter::default(), SortKey::ColorLabel, false);
        let wires: Vec<u8> = order
            .iter()
            .map(|id| label_order(board.item(*id).unwrap()))
            .collect();
        assert_eq!(
            wires,
            vec![
                0,
                ColorLabel::Red.to_wire(),
                ColorLabel::Blue.to_wire(),
                200
            ]
        );
    }

    #[test]
    fn the_default_filter_hides_nothing() {
        let (board, ids) = board_with((0..5).map(image_item).collect());
        assert!(Filter::default().is_open());
        assert_eq!(
            select(&board, &Filter::default(), SortKey::AddedAt, false),
            ids
        );
    }

    #[test]
    fn filtering_by_rating_keeps_only_what_clears_the_bar() {
        let mut board = Board::default();
        for stars in 0..=5u8 {
            let id = board.insert_item(image_item(stars));
            board
                .set_meta(
                    id,
                    ItemMeta {
                        rating: stars,
                        ..ItemMeta::default()
                    },
                )
                .unwrap();
        }
        let filter = Filter {
            min_rating: 3,
            ..Filter::default()
        };
        assert!(!filter.is_open());
        let kept = select(&board, &filter, SortKey::Rating, false);
        assert_eq!(kept.len(), 3, "ควรเหลือ 3, 4, 5 ดาว");
        for id in kept {
            assert!(board.item(id).unwrap().meta.rating >= 3);
        }
    }

    #[test]
    fn filtering_by_text_looks_at_the_file_name_the_note_and_the_text_item() {
        let mut board = Board::default();
        let picture = board.insert_item(image_named("Sunset-Sky.png", 10, 10));
        let with_note = board.insert_item(image_named("plain.png", 10, 10));
        board
            .set_meta(
                with_note,
                ItemMeta {
                    note: "ท้องฟ้ายามเย็น".to_owned(),
                    ..ItemMeta::default()
                },
            )
            .unwrap();
        let note_item = board.insert_item(Item::new(ItemKind::Text(TextNote {
            text: "sky study".to_owned(),
        })));
        let other = board.insert_item(image_named("cat.png", 10, 10));

        let by_name = Filter {
            // ★ ตัวพิมพ์ต่างกัน — ผู้ใช้ไม่ได้พิมพ์ตรงตัวพิมพ์เสมอ
            text: "sky".to_owned(),
            ..Filter::default()
        };
        let kept = select(&board, &by_name, SortKey::AddedAt, false);
        assert_eq!(kept, vec![picture, note_item]);

        let by_note = Filter {
            text: "ยามเย็น".to_owned(),
            ..Filter::default()
        };
        assert_eq!(
            select(&board, &by_note, SortKey::AddedAt, false),
            vec![with_note]
        );
        assert!(!matches(&board, other, &by_note));
    }

    /// ★ ค้นด้วยชื่อโฟลเดอร์ต้องไม่เจอทุกใบ — คำค้นดูแค่ **ชื่อไฟล์**
    #[test]
    fn searching_does_not_match_the_folder_the_file_lives_in() {
        let (board, _) = board_with(vec![
            image_named("C:/references/cat.png", 10, 10),
            image_named("C:/references/dog.png", 10, 10),
        ]);
        let filter = Filter {
            text: "references".to_owned(),
            ..Filter::default()
        };
        assert!(select(&board, &filter, SortKey::AddedAt, false).is_empty());
    }

    #[test]
    fn filtering_by_tags_covers_any_all_and_none() {
        let mut board = Board::default();
        let red = board.insert_tag("red").unwrap();
        let sky = board.insert_tag("sky").unwrap();
        let both = board.insert_item(image_item(1));
        let only_red = board.insert_item(image_item(2));
        let bare = board.insert_item(image_item(3));
        board
            .set_meta(
                both,
                ItemMeta {
                    tags: smallvec::smallvec![red, sky],
                    ..ItemMeta::default()
                },
            )
            .unwrap();
        board
            .set_meta(
                only_red,
                ItemMeta {
                    tags: smallvec::smallvec![red],
                    ..ItemMeta::default()
                },
            )
            .unwrap();

        let with = |tags: Vec<TagId>, mode: TagMode| Filter {
            tags: tags.into(),
            tag_mode: mode,
            ..Filter::default()
        };
        assert_eq!(
            select(
                &board,
                &with(vec![red, sky], TagMode::Any),
                SortKey::AddedAt,
                false
            ),
            vec![both, only_red]
        );
        assert_eq!(
            select(
                &board,
                &with(vec![red, sky], TagMode::All),
                SortKey::AddedAt,
                false
            ),
            vec![both]
        );
        assert_eq!(
            select(
                &board,
                &with(vec![sky], TagMode::None),
                SortKey::AddedAt,
                false
            ),
            vec![only_red, bare]
        );
    }

    #[test]
    fn filtering_by_colour_label_can_ask_for_the_unlabelled_ones() {
        let mut board = Board::default();
        let labelled = board.insert_item(image_item(1));
        let bare = board.insert_item(image_item(2));
        board
            .set_meta(
                labelled,
                ItemMeta {
                    color_label: Some(ColorLabel::Green),
                    ..ItemMeta::default()
                },
            )
            .unwrap();

        let is_green = Filter {
            label: LabelFilter::Is(ColorLabel::Green),
            ..Filter::default()
        };
        assert_eq!(
            select(&board, &is_green, SortKey::AddedAt, false),
            vec![labelled]
        );
        let unlabelled = Filter {
            label: LabelFilter::Unlabelled,
            ..Filter::default()
        };
        assert_eq!(
            select(&board, &unlabelled, SortKey::AddedAt, false),
            vec![bare]
        );
    }

    /// board ว่าง / id ที่ตายแล้ว ต้องไม่ panic
    #[test]
    fn nothing_breaks_on_an_empty_board_or_a_dead_id() {
        let board = Board::default();
        assert!(select(&board, &Filter::default(), SortKey::Name, false).is_empty());
        assert!(!matches(
            &board,
            ItemId::from_parts(9, 9),
            &Filter::default()
        ));
    }

    /// ★★ เรียงตาม **วันที่แก้ไขไฟล์** กับ **ขนาดไฟล์** — สองตัวที่เพิ่งปลดล็อก
    ///
    /// ข้อมูลถูกคำนวณอยู่แล้วตอน ingest (เป็นส่วนหนึ่งของ cache key) แค่เดิมถูกทิ้ง
    /// · `ModifiedAt` **ไม่ใช่** `AddedAt`: สแกนงานเก่าเข้ามาทั้งโฟลเดอร์วันนี้
    /// = เพิ่มพร้อมกันหมด แต่วันที่แก้ไขไฟล์ต่างกันเป็นปี
    #[test]
    fn sorting_by_file_date_and_size_uses_what_ingest_already_measured() {
        let mut board = Board::default();
        // (ชื่อ, mtime, ขนาด) — จงใจให้สามลำดับนี้ไม่ตรงกันเลยสักคู่
        for (name, mtime, bytes) in [
            ("new-small.png", 3_000i64, 10u64),
            ("old-big.png", 1_000, 30),
            ("mid.png", 2_000, 20),
        ] {
            let mut item = image_named(name, 10, 10);
            if let ItemKind::Image(asset) = &mut item.kind {
                asset.mtime = mtime;
                asset.file_size = bytes;
            }
            board.insert_item(item);
        }
        let by = |key| names(&board, &select(&board, &Filter::default(), key, false));
        assert_eq!(
            by(SortKey::ModifiedAt),
            vec!["old-big.png", "mid.png", "new-small.png"]
        );
        assert_eq!(
            by(SortKey::FileSize),
            vec!["new-small.png", "mid.png", "old-big.png"]
        );
        // ★ และต้องต่างจากลำดับที่เพิ่มเข้ามา ไม่งั้นเทสต์นี้ผ่านได้ฟรี ๆ
        assert_ne!(by(SortKey::ModifiedAt), by(SortKey::AddedAt));
        assert_ne!(by(SortKey::FileSize), by(SortKey::AddedAt));
    }

    /// โน้ต (ไม่มีไฟล์) ต้องไม่ทำให้การเรียงตามไฟล์พัง — ค่าศูนย์มาก่อน
    #[test]
    fn items_without_a_file_sort_first_by_file_date() {
        let mut board = Board::default();
        let note = board.insert_item(Item::new(ItemKind::Text(TextNote::default())));
        let mut picture = image_named("a.png", 10, 10);
        if let ItemKind::Image(asset) = &mut picture.kind {
            asset.mtime = 5_000;
        }
        let picture = board.insert_item(picture);
        assert_eq!(
            select(&board, &Filter::default(), SortKey::ModifiedAt, false),
            vec![note, picture]
        );
    }

    // ---------- P3-6: เรียงตามตำแหน่งบน canvas ----------

    /// วางภาพขนาด 100×80 ที่จุดกึ่งกลางที่กำหนด แล้วคืน id ตามลำดับที่วาง
    fn placed_at(spots: &[(f32, f32)]) -> (Board, Vec<ItemId>) {
        let mut board = Board::default();
        let ids = spots
            .iter()
            .enumerate()
            .map(|(i, (x, y))| {
                let item = image_named(&format!("{i}.png"), 100, 80)
                    .at(Vec2::new(*x, *y), Vec2::new(100.0, 80.0));
                board.insert_item(item)
            })
            .collect();
        (board, ids)
    }

    fn order_of(board: &Board, ids: &[ItemId], descending: bool) -> Vec<usize> {
        select(board, &Filter::default(), SortKey::CanvasOrder, descending)
            .into_iter()
            .map(|id| ids.iter().position(|other| *other == id).unwrap())
            .collect()
    }

    /// ★★★ อ่านแบบหนังสือ: บน→ล่าง ซ้าย→ขวา และ **ทนต่อการวางไม่ตรงแถว**
    ///
    /// mood board ไม่มีใครวางภาพให้ขอบบนตรงกันเป๊ะ — ถ้าเทียบ `y` ตรง ๆ
    /// ภาพสองใบที่ตาเห็นว่าอยู่แถวเดียวกันจะสลับลำดับกันเพราะต่างกันไม่กี่พิกเซล
    #[test]
    fn canvas_order_reads_like_a_page_even_when_rows_are_not_perfectly_aligned() {
        // แถวบนสามใบ (y เยื้องกันได้ถึง 20 ซึ่งน้อยกว่าครึ่งของความสูง 80)
        // แถวล่างสองใบ อยู่ห่างลงไปมาก
        let (board, ids) = placed_at(&[
            (300.0, 12.0),  // 0: แถวบน ขวาสุด
            (100.0, 0.0),   // 1: แถวบน ซ้ายสุด
            (900.0, 300.0), // 2: แถวล่าง ขวา
            (200.0, 20.0),  // 3: แถวบน กลาง
            (400.0, 292.0), // 4: แถวล่าง ซ้าย
        ]);
        assert_eq!(order_of(&board, &ids, false), vec![1, 3, 0, 4, 2]);
    }

    /// ★ ห่างเกินครึ่งของความสูงเฉลี่ย = คนละแถว แม้จะอยู่คอลัมน์เดียวกัน
    #[test]
    fn a_gap_bigger_than_half_the_average_height_starts_a_new_row() {
        // ความสูง 80 → ระยะยอมรับ 40
        let (board, ids) = placed_at(&[(0.0, 0.0), (500.0, 39.0), (500.0, 41.0)]);
        // ใบที่ 1 ยังอยู่แถวเดียวกับใบที่ 0 (ห่าง 39) ส่วนใบที่ 2 ขึ้นแถวใหม่
        assert_eq!(order_of(&board, &ids, false), vec![0, 1, 2]);

        let (board, ids) = placed_at(&[(500.0, 0.0), (0.0, 41.0)]);
        // ห่าง 41 = คนละแถว → ใบล่างมาทีหลังแม้จะอยู่ซ้ายกว่า
        assert_eq!(order_of(&board, &ids, false), vec![0, 1]);
    }

    /// ★★ ภาพที่ไล่ระดับลงมาทีละนิดต้องไม่กลายเป็นแถวเดียวยาวเหยียด
    ///
    /// ถ้าใช้ค่าเฉลี่ยที่เลื่อนตามแทนหมุดของแถว ใบแรกกับใบสุดท้ายจะห่างกันครึ่งจอ
    /// แต่ยังถูกนับเป็นแถวเดียวกัน
    #[test]
    fn a_staircase_does_not_collapse_into_one_row() {
        let spots: Vec<(f32, f32)> = (0..10u8)
            .map(|i| (f32::from(i) * 10.0, f32::from(i) * 30.0))
            .collect();
        let (board, ids) = placed_at(&spots);
        // แต่ละใบห่างกัน 30 (< 40) แต่สะสมแล้ว 270 — ต้องไม่ใช่แถวเดียว
        let order = order_of(&board, &ids, false);
        assert_eq!(order, (0..10).collect::<Vec<_>>());
        // ★★ เคสที่แยก "แถวเดียว" ออกจาก "หลายแถว" ได้จริง: ให้ x สวนทางกับ y
        //   ถ้ายุบเป็นแถวเดียว ผลจะเป็นการเรียงตาม x ล้วน = [9, 8, …, 0]
        let spots: Vec<(f32, f32)> = (0..10u8)
            .map(|i| (300.0 - f32::from(i) * 10.0, f32::from(i) * 30.0))
            .collect();
        let (board, ids) = placed_at(&spots);
        let order = order_of(&board, &ids, false);
        let pure_x: Vec<usize> = (0..10).rev().collect();
        assert_ne!(order, pure_x, "ยุบเป็นแถวเดียวแล้วเรียงตาม x ล้วน");
        // ★ ใบท้าย ๆ (อยู่ล่างสุด) ต้องมาหลังใบแรก ๆ เสมอ — นั่นคือ "อ่านลงล่าง"
        let position = |item: usize| order.iter().position(|i| *i == item).unwrap();
        assert!(position(9) > position(0), "ใบล่างสุดมาก่อนใบบนสุด");
        assert!(position(8) > position(1));

        // ★ และผลจริงคือจับคู่ทีละสองแถวตามระยะ 40 (30 อยู่ในระยะ · 60 ไม่อยู่)
        //   ในแถวเดียวกันเรียงซ้ายไปขวา ซึ่งที่นี่คือใบที่ y มากกว่า (x น้อยกว่า)
        assert_eq!(order, vec![1, 0, 3, 2, 5, 4, 7, 6, 9, 8]);
    }

    /// กลับทิศ = อ่านย้อนจากล่างขวา
    #[test]
    fn reversing_canvas_order_reads_from_the_bottom_right() {
        let (board, ids) = placed_at(&[(100.0, 0.0), (300.0, 0.0), (100.0, 300.0)]);
        assert_eq!(order_of(&board, &ids, false), vec![0, 1, 2]);
        assert_eq!(order_of(&board, &ids, true), vec![2, 1, 0]);
    }

    /// ผลต้องคงที่ และไม่พังกับ board ว่าง/ใบเดียว
    #[test]
    fn canvas_order_is_deterministic_and_survives_edge_cases() {
        let empty = Board::default();
        assert!(select(&empty, &Filter::default(), SortKey::CanvasOrder, false).is_empty());

        let (board, ids) = placed_at(&[(5.0, 5.0)]);
        assert_eq!(order_of(&board, &ids, false), vec![0]);

        let (board, ids) = placed_at(&[(100.0, 0.0), (0.0, 0.0), (50.0, 0.0)]);
        let first = order_of(&board, &ids, false);
        for _ in 0..5 {
            assert_eq!(order_of(&board, &ids, false), first);
        }
    }

    /// ★ ภาพที่ทับกันสนิทต้องไม่สลับที่กันเองเวลากดสลับทิศ
    #[test]
    fn items_stacked_on_top_of_each_other_keep_their_order_both_ways() {
        let (board, ids) = placed_at(&[(0.0, 0.0), (0.0, 0.0), (0.0, 0.0)]);
        assert_eq!(order_of(&board, &ids, false), vec![0, 1, 2]);
        assert_eq!(order_of(&board, &ids, true), vec![0, 1, 2]);
    }

    /// ★ ผลต้องเหมือนเดิมเป๊ะทุกครั้ง (docs/03 §3)
    #[test]
    fn the_same_board_always_yields_the_same_order() {
        let (board, _) = board_with((0..40).map(image_item).collect());
        let first = select(&board, &Filter::default(), SortKey::Name, false);
        for _ in 0..5 {
            assert_eq!(
                select(&board, &Filter::default(), SortKey::Name, false),
                first
            );
        }
    }
}
