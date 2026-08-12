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
    rows.into_iter().map(|(_, id)| id).collect()
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
