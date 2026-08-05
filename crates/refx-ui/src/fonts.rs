//! ฟอนต์ที่ฝังมากับ binary
//!
//! spec: docs/03-modes-and-ui.md §0, docs/06-security.md §2.5
//!
//! ฝัง **Latin (ของ egui เดิม) + Noto Sans Thai (OFL-1.1)** เท่านั้น
//! ยังไม่รองรับ CJK — ฟอนต์ครบชุด 16+ MB ทะลุเพดาน binary 25 MB (docs/08 §6)
//!
//! ### กติกาข้อเดียวที่สำคัญที่สุดของไฟล์นี้
//!
//! **ฟอนต์พังต้องไม่ทำให้โปรแกรมเปิดไม่ขึ้น** โปรแกรมที่ตัวหนังสือไทยเป็น `□`
//! ยังใช้งานได้ (UI หลักเป็นอังกฤษอยู่แล้ว) แต่โปรแกรมที่เปิดไม่ขึ้นคือใช้ไม่ได้เลย
//! — ลำดับความสำคัญข้อ 1 "เสถียร" มาก่อนข้อ 4 "ฟีเจอร์" เสมอ
//!
//! จึงมีเกราะสองชั้น: ตรวจโครงสร้างไฟล์เองก่อนส่งให้ egui แล้วห่อการ parse จริง
//! ของ egui ด้วย `catch_unwind` อีกที ถ้าชั้นไหนไม่ผ่านก็ถอยไปใช้ฟอนต์เดิม

use std::panic::AssertUnwindSafe;
use std::sync::Arc;

/// ชื่อที่ใช้อ้างถึงฟอนต์ไทยใน `FontDefinitions`
const THAI_FONT_NAME: &str = "noto-sans-thai";

/// ฟอนต์ไทยที่ฝังมาใน binary
///
/// ที่มา ลายเซ็น SHA-256 และ license อยู่ใน `assets/fonts/CHECKSUMS.txt`
/// และ `THIRD-PARTY-LICENSES.md` ที่รากโปรเจกต์
///
/// ★ `.gitattributes` ประกาศ `*.ttf binary` ไว้แล้ว — ถ้าไม่มีบรรทัดนั้น
/// git อาจ normalize line ending ในไฟล์นี้แล้วฟอนต์เสียแบบเงียบ ๆ
const THAI_FONT: &[u8] = include_bytes!("../../../assets/fonts/NotoSansThai-Regular.ttf");

/// ตรวจว่าไบต์ชุดนี้ยัง "เป็นฟอนต์" อยู่ไหม ก่อนส่งให้ egui
///
/// ไม่ได้ parse ทั้งไฟล์ — แค่ยืนยันว่าหัวไฟล์กับตาราง table directory สมเหตุสมผล
/// ซึ่งจับกรณีที่เกิดจริงได้: ไฟล์ถูกตัด, git แปลง line ending, checkout ผิดพลาด
///
/// เขียนเองเพราะการเพิ่ม dependency สำหรับ parse ฟอนต์ ต้องขออนุญาตก่อน (CLAUDE.md)
/// และการตรวจแค่นี้ก็ปิดช่องที่เกิดจริงได้เกือบหมดแล้ว
fn looks_like_a_font(bytes: &[u8]) -> bool {
    // sfnt version: TrueType คือ 0x00010000, OpenType/CFF คือ "OTTO", เก่ากว่านั้นคือ "true"
    let Some(tag) = bytes.get(..4) else {
        return false;
    };
    if !matches!(tag, [0x00, 0x01, 0x00, 0x00] | b"OTTO" | b"true") {
        return false;
    }

    // จำนวนตารางต้องสมเหตุสมผล และ table directory ต้องอยู่ในไฟล์จริง
    let Some(raw) = bytes.get(4..6) else {
        return false;
    };
    let num_tables = usize::from(u16::from_be_bytes([raw[0], raw[1]]));
    if num_tables == 0 || num_tables > 512 {
        return false;
    }
    // header 12 ไบต์ + 16 ไบต์ต่อหนึ่งรายการ
    let Some(directory_end) = num_tables.checked_mul(16).and_then(|n| n.checked_add(12)) else {
        return false;
    };
    if bytes.len() < directory_end {
        return false;
    }

    // ★ ทุกตารางต้องอยู่ในไฟล์จริง — นี่คือด่านที่จับ "ไฟล์ถูกตัด" ได้
    //
    //   table directory อยู่ต้นไฟล์ทั้งหมด ไฟล์ที่ถูกตัดครึ่งจึงยังมี directory ครบ
    //   และผ่านการตรวจแค่หัวไฟล์ไปได้สบาย ๆ ต้องไล่ดู offset+length ของทุกตาราง
    //   ว่ายังชี้ไปในไฟล์อยู่ไหม (แต่ละรายการ: tag 4 · checksum 4 · offset 4 · length 4)
    let mut has_cmap = false;
    let mut has_head = false;
    for index in 0..num_tables {
        let start = 12 + index * 16;
        let Some(record) = bytes.get(start..start + 16) else {
            return false;
        };
        let tag = &record[..4];
        let offset = u32::from_be_bytes([record[8], record[9], record[10], record[11]]) as usize;
        let length = u32::from_be_bytes([record[12], record[13], record[14], record[15]]) as usize;

        let Some(end) = offset.checked_add(length) else {
            return false;
        };
        if end > bytes.len() {
            return false; // ตารางนี้ชี้ออกนอกไฟล์ = ไฟล์ไม่ครบ
        }

        has_cmap |= tag == b"cmap";
        has_head |= tag == b"head";
    }

    // ไม่มีสองตารางนี้ = egui parse ไม่ผ่านแน่นอน (cmap คือตารางแปลงอักขระ→glyph)
    has_cmap && has_head
}

/// รายการฟอนต์ที่มีไทยต่อ **ท้าย** ทั้ง `Proportional` และ `Monospace`
///
/// ต่อท้าย ไม่ใช่แทนที่ — ละตินจึงยังใช้ฟอนต์ที่ออกแบบมาคู่กับ egui
/// แล้วอักษรไทยค่อยตกมาที่ฟอนต์ใหม่
fn definitions_with_thai() -> egui::FontDefinitions {
    let mut fonts = egui::FontDefinitions::default();
    fonts.font_data.insert(
        THAI_FONT_NAME.to_owned(),
        Arc::new(egui::FontData::from_static(THAI_FONT)),
    );
    for family in [egui::FontFamily::Proportional, egui::FontFamily::Monospace] {
        fonts
            .families
            .entry(family)
            .or_default()
            .push(THAI_FONT_NAME.to_owned());
    }
    fonts
}

/// ★ ลอง parse จริงบน `Context` **ตัวชั่วคราว** ก่อนเอาไปใช้กับของจริง
///
/// สองเรื่องที่บังคับให้ต้องทำแบบนี้ ไม่ใช่ parse บน context จริงตรง ๆ:
///
/// 1. `ctx.fonts_mut()` **panic** ถ้าเรียกก่อนที่ context จะรัน pass แรก
///    (`"No fonts available until first call to Context::run()"`)
/// 2. การรัน pass เพื่อบังคับ parse จะผลิต `textures_delta` ที่มี font atlas อยู่
///    ถ้าเราทิ้งผลนั้น egui จะถือว่าส่ง texture ไปแล้ว แต่ renderer ไม่เคยได้รับ
///    → **ตัวหนังสือหายทั้งจอ** ซึ่งแย่กว่าปัญหาที่กำลังแก้อยู่มาก
///
/// context ชั่วคราวไม่มีใครวาด จึงทิ้ง delta ได้อย่างปลอดภัย
/// ต้นทุนคือสร้าง font atlas ทิ้งหนึ่งครั้งตอนเปิดโปรแกรม แลกกับการรู้ผลก่อน
fn parses_without_panicking(fonts: &egui::FontDefinitions) -> bool {
    let probe = egui::Context::default();
    probe.set_fonts(fonts.clone());
    std::panic::catch_unwind(AssertUnwindSafe(|| {
        // pass เปล่าหนึ่งรอบเพื่อให้ egui สร้าง font atlas จริง
        let _ = probe.run_ui(egui::RawInput::default(), |_| {});
        probe.fonts_mut(|fonts| fonts.has_glyph(&egui::FontId::proportional(14.0), 'ก'))
    }))
    .unwrap_or(false)
}

/// ติดตั้งฟอนต์ทั้งชุดให้ `Context`
///
/// เรียกครั้งเดียวตอนสร้าง context (และอีกครั้งหลังกู้ device เพราะ P0-5
/// สร้าง `egui::Context` ใหม่ทั้งก้อน — docs/04 §7 ข้อ 3)
///
/// ★ ไม่ว่าจะพังยังไงก็ **คืนค่าปกติ** ไม่ panic — ฟอนต์พังต้องไม่ทำให้เปิดโปรแกรมไม่ได้
pub fn install(ctx: &egui::Context) {
    // เกราะชั้น 1: ไฟล์ในโฟลเดอร์ assets เสียหาย (ถูกตัด, line ending โดนแปลง)
    if !looks_like_a_font(THAI_FONT) {
        tracing::error!(
            bytes = THAI_FONT.len(),
            "the bundled Thai font is not a valid font file — falling back to egui's own font (Thai text renders as boxes, everything else keeps working)"
        );
        return;
    }

    // เกราะชั้น 2: โครงสร้างถูกแต่ egui อ่านไม่ได้จริง
    let fonts = definitions_with_thai();
    if !parses_without_panicking(&fonts) {
        tracing::error!(
            "egui rejected the Thai font — falling back to egui's own font (Thai text renders as boxes, everything else keeps working)"
        );
        return;
    }

    ctx.set_fonts(fonts);
    tracing::info!(
        font = THAI_FONT_NAME,
        bytes = THAI_FONT.len(),
        "Thai font installed"
    );
}

/// ฟอนต์ที่ติดตั้งอยู่มี glyph ของตัวอักษรนี้ไหม
///
/// ใช้ในเทสต์เพื่อพิสูจน์ว่าฟอนต์ไทย **ถูกใช้จริง** ไม่ใช่แค่ถูกใส่เข้าไปในรายการ
#[must_use]
pub fn has_glyph(ctx: &egui::Context, ch: char) -> bool {
    let font_id = egui::FontId::proportional(14.0);
    ctx.fonts_mut(|fonts| fonts.has_glyph(&font_id, ch))
}

/// ทุกตัวอักษรในข้อความนี้มี glyph ครบไหม
#[must_use]
pub fn has_glyphs(ctx: &egui::Context, text: &str) -> bool {
    let font_id = egui::FontId::proportional(14.0);
    ctx.fonts_mut(|fonts| fonts.has_glyphs(&font_id, text))
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::float_cmp)]

    use super::*;

    /// context ที่ติดตั้งฟอนต์แล้วและรัน pass ไปหนึ่งรอบ — egui ไม่ต้องมี GPU
    ///
    /// ต้องรัน pass ก่อน ไม่งั้น `fonts_mut()` จะ panic ว่า
    /// "No fonts available until first call to Context::run()"
    fn ctx_with_fonts() -> egui::Context {
        let ctx = egui::Context::default();
        install(&ctx);
        let _ = ctx.run_ui(egui::RawInput::default(), |_| {});
        ctx
    }

    // ---------- ตัวไฟล์ฟอนต์ ----------

    #[test]
    fn embedded_font_is_a_real_font_file() {
        assert!(
            looks_like_a_font(THAI_FONT),
            "ฟอนต์ที่ฝังมาไม่ผ่านการตรวจโครงสร้าง ({} ไบต์)",
            THAI_FONT.len()
        );
        // ถ้า git normalize line ending ขนาดจะเปลี่ยน — ตรึงไว้ให้เห็นทันที
        assert_eq!(
            THAI_FONT.len(),
            37_780,
            "ขนาดฟอนต์ไม่ตรงกับที่บันทึกใน assets/fonts/CHECKSUMS.txt"
        );
    }

    /// ★ ไฟล์เสียทุกแบบต้องได้ `false` ไม่ใช่ panic — นี่คือเกราะชั้นแรก
    #[test]
    fn broken_font_files_are_rejected_not_panicked() {
        let cases: Vec<Vec<u8>> = vec![
            Vec::new(),
            b"\x00".to_vec(),
            b"\x00\x01\x00".to_vec(),                  // สั้นกว่า sfnt version
            b"not a font at all".to_vec(),             // tag ผิด
            b"\x00\x01\x00\x00".to_vec(),              // tag ถูกแต่ไม่มีอะไรต่อ
            b"\x00\x01\x00\x00\x00\x00".to_vec(),      // 0 ตาราง
            b"\x00\x01\x00\x00\xff\xff".to_vec(),      // 65535 ตาราง (เกินจริง)
            THAI_FONT[..12].to_vec(),                  // มีแต่ header
            THAI_FONT[..THAI_FONT.len() / 2].to_vec(), // ถูกตัดครึ่ง
        ];
        for (i, case) in cases.iter().enumerate() {
            assert!(!looks_like_a_font(case), "เคส {i} ควรถูกปฏิเสธ");
        }
    }

    /// ตัดฟอนต์จริงทุกความยาว — ต้องไม่ panic สักจุด
    #[test]
    fn truncating_the_real_font_never_panics() {
        for cut in (0..THAI_FONT.len()).step_by(97) {
            let _ = looks_like_a_font(&THAI_FONT[..cut]);
        }
    }

    // ---------- ★ glyph ไทยต้องมีจริง ----------

    /// ตัวอักษรพื้นฐาน สระ และวรรณยุกต์ ต้องมี glyph ครบ
    ///
    /// ★ ทดสอบ **สระบน/ล่างและวรรณยุกต์แยกต่างหาก** เพราะฟอนต์ที่มีแต่พยัญชนะ
    /// จะผ่านการตรวจแบบหยาบ ๆ ได้ แต่ข้อความจริงอย่าง "กำลังโหลด" จะขึ้นไม่ครบ
    #[test]
    fn thai_glyphs_exist_including_marks_and_tones() {
        let ctx = ctx_with_fonts();

        let groups: [(&str, &str); 4] = [
            ("พยัญชนะ", "กขคงจฉชญฎฏฐณดตถทธนบปผพภมยรลวศษสหอฮ"),
            ("สระหน้า/หลัง", "เแโใไะาๆ"),
            ("สระบน/ล่าง วรรณยุกต์", "ิีึืุู่้๊๋ั็์"),
            ("ตัวเลขไทย", "๐๑๒๓๔๕๖๗๘๙"),
        ];
        for (label, chars) in groups {
            for ch in chars.chars() {
                assert!(
                    has_glyph(&ctx, ch),
                    "{label}: ไม่มี glyph ของ {ch:?} (U+{:04X})",
                    ch as u32
                );
            }
        }
    }

    /// ★ ข้อความจริงที่ผู้ใช้เห็นต้องวางได้ครบทุกตัวอักษร
    ///
    /// "กำลังโหลด" มีทั้ง สระอำ (นิคหิต+สระอา) สระอิ ไม้หันอากาศ และสระโอ
    /// ซึ่งเป็นชุดที่ฟอนต์ไม่ครบจะแตกทันที
    /// ★★ ป้ายบนปุ่มจัดเรียงต้องมี glyph จริงในฟอนต์ที่เราฝัง
    ///
    /// เคยพลาดมาแล้วตอน P2-9: เลือกสัญลักษณ์เส้นตาราง (`┣ ┫ ┳ ┻`) ที่ดูเหมาะที่สุด
    /// แล้วปุ่มขึ้นเป็น **กล่องสี่เหลี่ยมว่าง** บนจอจริง เพราะฟอนต์ที่ฝังไม่มี glyph
    /// — ไม่มี error ไม่มี log เห็นได้ทางเดียวคือเปิดโปรแกรมแล้วดู
    ///
    /// `↔`/`↕` ใช้ได้แต่ `← → ↑ ↓` ใช้ไม่ได้ ซึ่ง **เดาไม่ได้เลย** ต้องตรวจ
    #[test]
    fn arrange_button_labels_all_have_glyphs() {
        let ctx = ctx_with_fonts();
        for (_, label, _) in crate::shell::ARRANGE_BUTTONS {
            for ch in label.chars() {
                assert!(
                    has_glyph(&ctx, ch),
                    "ป้ายปุ่ม {label:?} ไม่มี glyph ของ {ch:?} (U+{:04X})                      — มันจะขึ้นเป็นกล่องว่างบนจอ",
                    ch as u32
                );
            }
        }
    }

    #[test]
    fn real_ui_strings_lay_out_completely() {
        use crate::text::{self, Key, Lang, Template};

        let ctx = ctx_with_fonts();
        let samples = [
            text::fill(
                Lang::Th,
                Template::Loading,
                &[("done", "312"), ("total", "1000")],
            ),
            text::t(Lang::Th, Key::Ready).to_owned(),
            text::t(Lang::Th, Key::LibraryPlaceholder).to_owned(),
            text::fill(Lang::Th, Template::ErrUnknownFormat, &[]),
        ];

        for sample in samples {
            // ข้ามช่องว่างและตัวขึ้นบรรทัด — ไม่มีฟอนต์ไหนมี glyph ให้ตัวพวกนี้
            // (ข้อความ error ของเราเป็นสองบรรทัดเสมอตามกฎใน CLAUDE.md)
            for ch in sample.chars().filter(|c| !c.is_whitespace()) {
                assert!(
                    has_glyph(&ctx, ch),
                    "ข้อความ {sample:?} วางไม่ครบ — ไม่มี glyph ของ {ch:?} (U+{:04X})",
                    ch as u32
                );
            }
        }
    }

    /// ละตินต้องยังใช้ฟอนต์เดิมของ egui ไม่ถูกฟอนต์ไทยแทนที่
    #[test]
    fn latin_still_renders_after_adding_thai() {
        let ctx = ctx_with_fonts();
        assert!(
            has_glyphs(&ctx, "RefX Library Inspector 0123456789"),
            "ละตินหายหลังเพิ่มฟอนต์ไทย"
        );
    }

    /// ฟอนต์ไทยต้องอยู่ **ท้าย** รายการทั้งสองตระกูล ไม่ใช่หัวรายการ
    #[test]
    fn thai_font_is_appended_not_prepended() {
        let mut fonts = egui::FontDefinitions::default();
        let before: Vec<String> = fonts.families[&egui::FontFamily::Proportional].clone();

        fonts.font_data.insert(
            THAI_FONT_NAME.to_owned(),
            Arc::new(egui::FontData::from_static(THAI_FONT)),
        );
        for family in [egui::FontFamily::Proportional, egui::FontFamily::Monospace] {
            fonts
                .families
                .entry(family)
                .or_default()
                .push(THAI_FONT_NAME.to_owned());
        }

        let after = &fonts.families[&egui::FontFamily::Proportional];
        assert_eq!(after.last().map(String::as_str), Some(THAI_FONT_NAME));
        assert_eq!(
            &after[..before.len()],
            &before[..],
            "ของเดิมต้องอยู่ครบและอยู่ก่อน"
        );
        assert!(
            fonts.families[&egui::FontFamily::Monospace]
                .iter()
                .any(|name| name == THAI_FONT_NAME),
            "Monospace ต้องมีฟอนต์ไทยด้วย ไม่งั้นข้อความ mono ภาษาไทยจะเป็นสี่เหลี่ยม"
        );
    }

    /// ติดตั้งซ้ำได้ (เกิดจริงทุกครั้งที่กู้ device — P0-5 สร้าง Context ใหม่)
    #[test]
    fn installing_twice_is_fine() {
        let ctx = egui::Context::default();
        install(&ctx);
        install(&ctx);
        let _ = ctx.run_ui(egui::RawInput::default(), |_| {});
        assert!(has_glyph(&ctx, 'ก'));
    }

    /// ★ เกราะชั้น 2 ต้องผ่านกับฟอนต์จริง
    ///
    /// ถ้าข้อนี้ล้ม แปลว่า `install` จะถอยไปใช้ฟอนต์เดิม **ทุกครั้ง** อย่างเงียบ ๆ
    /// แล้วตัวหนังสือไทยจะเป็นสี่เหลี่ยมทั้งที่ฝังฟอนต์มาถูกต้อง
    /// (เกิดขึ้นจริงตอนเขียนครั้งแรก — `fonts_mut()` panic ก่อนรัน pass แรก)
    #[test]
    fn the_real_font_passes_the_parse_probe() {
        assert!(
            parses_without_panicking(&definitions_with_thai()),
            "ฟอนต์จริงไม่ผ่าน probe — install จะถอยกลับไปใช้ฟอนต์เดิมแบบเงียบ ๆ"
        );
    }

    /// ไบต์ที่ไม่ใช่ฟอนต์ต้องทำให้ probe ตอบ false ไม่ใช่พาโปรแกรมล้ม
    #[test]
    fn probe_rejects_garbage_without_panicking() {
        let mut fonts = egui::FontDefinitions::default();
        fonts.font_data.insert(
            "ขยะ".to_owned(),
            Arc::new(egui::FontData::from_static(
                b"not a font at all, just bytes",
            )),
        );
        fonts
            .families
            .entry(egui::FontFamily::Proportional)
            .or_default()
            .push("ขยะ".to_owned());
        assert!(!parses_without_panicking(&fonts));
    }
}
