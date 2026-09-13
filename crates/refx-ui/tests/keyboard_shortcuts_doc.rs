//! ★★★ คู่มือคีย์ลัด **สร้างจากตารางจริง ไม่ใช่พิมพ์มือ** (P5-7)
//!
//! ## ทำไมต้องสร้าง
//!
//! คู่มือที่พิมพ์มือจะผิดตั้งแต่วันแรกที่มีคนแก้ `keymap.toml` หรือเพิ่ม binding
//! และมันผิดแบบ **เงียบ** — ไม่มีอะไรแดง มีแต่ผู้ใช้ที่กดตามคู่มือแล้วไม่เกิดอะไร
//! รูปเดียวกับ `THIRD-PARTY-LICENSES.md` ที่ล้าสมัยเงียบ ๆ ทุกครั้งที่ lock ขยับ
//!
//! ## ★★ แหล่งความจริงคือตัวเดียวกับที่ UI แสดง
//!
//! ใช้ `chord.display()` กับ `action.name()` — **สองตัวเดียวกับที่แผง Settings
//! เรียก** · ถ้าเอกสารกับแผงต่างกันเมื่อไหร่ แปลว่ามีสองแหล่งความจริงแล้ว
//!
//! ## ใช้
//!
//! ```text
//! cargo nextest run -p refx-ui -E 'test(keyboard_shortcuts)'   # ประตู
//! REFX_BLESS=1 cargo test -p refx-ui --test keyboard_shortcuts_doc  # เขียนใหม่
//! ```

// ★ เหตุผลเดียวกับเทสต์อื่นในโฟลเดอร์นี้: นี่คือเครื่องมือที่อ่าน/เขียนไฟล์ของ
//   โปรเจกต์เอง ไม่ใช่โค้ดที่รันบน UI thread ของผู้ใช้ (I-2 ไม่เกี่ยว)
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::disallowed_methods)]

use std::path::PathBuf;

use refx_ui::keymap;

/// ไฟล์ปลายทาง — อยู่ที่รากเพราะเป็น **เอกสารของผู้ใช้** ไม่ใช่สเปก
///
/// ★ `docs/` เป็นของเจ้าของโปรเจกต์ (สเปก) · คู่มือที่เครื่องสร้างจึงไม่ควรไปอยู่
/// ปนกับของที่คนเขียนด้วยมือ — รูปเดียวกับ `THIRD-PARTY-LICENSES.md`
const OUTPUT: &str = "KEYBOARD-SHORTCUTS.md";

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("หารากของ workspace ไม่เจอ")
        .to_path_buf()
}

/// ประกอบเอกสารจากตาราง `BUILTIN` ที่โปรแกรมใช้จริง
fn render() -> String {
    let bindings = keymap::builtin().bindings();
    let mut out = String::new();
    out.push_str("<!-- ★ สร้างโดยเทสต์ `keyboard_shortcuts_doc` — ห้ามแก้ด้วยมือ -->\n");
    out.push_str(
        "<!-- เขียนใหม่: REFX_BLESS=1 cargo test -p refx-ui --test keyboard_shortcuts_doc -->\n\n",
    );
    out.push_str("# คีย์ลัดของ RefX / Keyboard shortcuts\n\n");
    out.push_str(
        "ตารางนี้ถูกสร้างจากตารางคีย์ลัดที่โปรแกรมใช้จริง จึงตรงกับรุ่นที่คุณถืออยู่เสมอ\n\n\
         This table is generated from the binding table the program actually uses, \
         so it always matches the build you have.\n\n",
    );
    out.push_str(
        "เปลี่ยนเองได้ที่ `keymap.toml` ในโฟลเดอร์ตั้งค่า — ดูแผง Settings ในโปรแกรม\n\n\
         You can remap these in `keymap.toml` in the settings folder; see the \
         Settings panel in the app.\n\n",
    );

    // ★ นับก่อนพิมพ์ — ตัวเลขที่อยู่ในเอกสารทำให้ "หายไปหนึ่งแถว" อ่านออก
    //   โดยไม่ต้องนับแถวเอง (`docs/08 §3.9` ข้อ 9)
    let rows: Vec<(String, &str)> = bindings
        .iter()
        // ★ `display()` คืน `None` ให้ alias ของอักขระควบคุม (เช่น `\u{1a}` ของ
        //   Ctrl+Z) — มันเป็นแถวเดียวกับปุ่มที่ผู้ใช้กด ไม่ใช่คีย์ลัดคนละตัว
        //   และไม่มี glyph ในฟอนต์ที่ฝังไว้ · UI ก็กรองมันออกด้วยเหตุผลเดียวกัน
        .filter_map(|b| Some((b.chord.display()?, b.action.name())))
        .collect();
    out.push_str(&format!(
        "**{} คีย์ลัด / {} shortcuts**\n\n",
        rows.len(),
        rows.len()
    ));
    out.push_str("| ปุ่ม / Keys | สั่งอะไร / Action |\n|---|---|\n");
    for (chord, action) in rows {
        out.push_str(&format!("| `{chord}` | {action} |\n"));
    }
    out
}

/// ★★★ ประตู: เอกสารต้องตรงกับตารางเสมอ
///
/// `REFX_BLESS=1` = เขียนใหม่ (ใช้ตอนตั้งใจเปลี่ยนคีย์ลัด) · ไม่ตั้ง = ตรวจ
#[test]
fn keyboard_shortcuts_doc_matches_the_real_table() {
    let doc = render();
    let path = root().join(OUTPUT);

    if std::env::var_os("REFX_BLESS").is_some() {
        std::fs::write(&path, doc.as_bytes()).expect("เขียนไฟล์ไม่ได้");
        println!("เขียน {OUTPUT} ใหม่แล้ว ({} ไบต์)", doc.len());
        return;
    }

    let found = std::fs::read_to_string(&path).unwrap_or_default();
    assert_eq!(
        found.replace("\r\n", "\n"),
        doc,
        "\n★★★ {OUTPUT} ไม่ตรงกับตารางคีย์ลัดจริง\n\
         คีย์ลัดเปลี่ยนแล้วแต่คู่มือยังเป็นของเก่า — ผู้ใช้ที่กดตามคู่มือจะพบว่า\n\
         ไม่เกิดอะไรขึ้น และไม่มีอะไรบอกเขาว่าคู่มือผิด\n\
         → รัน: REFX_BLESS=1 cargo test -p refx-ui --test keyboard_shortcuts_doc\n"
    );
}

/// ★★ ประตูของประตู — เอกสารที่ว่างเปล่าก็ "ตรงกัน" ได้ถ้าตารางว่างเปล่า
///
/// และแถวที่ไม่มีปุ่มหรือไม่มีชื่อ action คือแถวที่ผู้ใช้อ่านแล้วไม่ได้อะไร
#[test]
fn the_generated_table_actually_has_rows_worth_reading() {
    let doc = render();
    let rows = doc.lines().filter(|l| l.starts_with("| `")).count();
    assert!(rows >= 20, "ตารางมีแค่ {rows} แถว — ตัวสร้างน่าจะกรองทิ้งเกินไป");

    for binding in keymap::builtin().bindings() {
        assert!(
            !binding.action.name().trim().is_empty(),
            "มี action ที่ไม่มีชื่อ — แถวนั้นในคู่มือจะว่างเปล่า"
        );
    }
    // ★ ปุ่มที่ผู้ใช้คาดหวังแน่ ๆ ต้องอยู่ในนั้นจริง ไม่ใช่แค่ "มีหลายแถว"
    for must in ["ctrl+z", "ctrl+s", "ctrl+v"] {
        assert!(
            doc.contains(&format!("| `{must}` |")),
            "ไม่มี {must} ในคู่มือ — ตัวสร้างหรือตารางผิด"
        );
    }
}
