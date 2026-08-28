//! ตัวเขียน JSON แบบสตรีมขนาดจิ๋ว — เขียนเองเพราะ **`serde_json` ไม่อยู่ใน
//! `docs/09`** และการเพิ่ม dependency ต้องถามก่อน (`CLAUDE.md`)
//!
//! สิ่งที่มันต้องทำถูกมีแค่สามข้อ และทั้งสามข้อมีเทสต์คุม:
//!
//! 1. **escape ให้ครบ** — ชื่อ board / path / โน้ต มาจากไฟล์ของผู้ใช้ (I-4)
//!    ถ้า escape ไม่ครบ dump จะพ่น JSON ที่ parser อื่นอ่านไม่ออก หรือแย่กว่านั้น
//!    คือ **อ่านออกแต่ได้เนื้อผิด**
//! 2. **ค่า float ที่ไม่ finite ต้องไม่พ่นออกมาดิบ ๆ** — JSON ไม่มี `NaN`/`Infinity`
//!    และค่าพวกนั้นมาถึงเราได้จริง เพราะไฟล์โกหกได้ (`decode_document` ไม่ผ่าน
//!    `sanitized()` ซึ่งเป็นเจตนา: dump ต้องบอกว่า *ในไฟล์* มีอะไร)
//!    → เขียนเป็น **สตริง** `"NaN"` / `"Infinity"` / `"-Infinity"`
//! 3. **สตรีมออกไปเลย** ไม่สะสมสตริงทั้งก้อนไว้ใน RAM

use std::io::{self, Write};

/// ตัวเขียน JSON ที่จัดคอมมา/ย่อหน้าให้เอง
///
/// ผู้เรียกเปิด/ปิด container เอง — โครงผิดจะเห็นทันทีในผลลัพธ์
pub struct JsonWriter<W: Write> {
    out: W,
    /// หนึ่งช่องต่อ container ที่เปิดค้างอยู่ — `true` = ยังไม่มีสมาชิกเลย
    empty: Vec<bool>,
}

impl<W: Write> JsonWriter<W> {
    /// สร้างตัวเขียนใหม่
    pub fn new(out: W) -> Self {
        Self {
            out,
            empty: Vec::new(),
        }
    }

    /// จบการเขียน — คืนปลายทางกลับให้ผู้เรียก
    ///
    /// # Errors
    /// เมื่อ flush ไม่สำเร็จ
    pub fn finish(mut self) -> io::Result<W> {
        self.out.write_all(b"\n")?;
        self.out.flush()?;
        Ok(self.out)
    }

    /// คอมมา + ขึ้นบรรทัด + ย่อหน้า ก่อนสมาชิกตัวถัดไป
    fn sep(&mut self) -> io::Result<()> {
        if let Some(empty) = self.empty.last_mut() {
            if *empty {
                *empty = false;
            } else {
                self.out.write_all(b",")?;
            }
            self.out.write_all(b"\n")?;
            for _ in 0..self.empty.len() {
                self.out.write_all(b"  ")?;
            }
        }
        Ok(())
    }

    /// เปิด object — ต้องตามหลัง [`Self::key`] / [`Self::next`] เสมอ (ยกเว้นตัวนอกสุด)
    ///
    /// # Errors
    /// เมื่อเขียนไม่สำเร็จ
    pub fn begin_object(&mut self) -> io::Result<()> {
        self.out.write_all(b"{")?;
        self.empty.push(true);
        Ok(())
    }

    /// ปิด object
    ///
    /// # Errors
    /// เมื่อเขียนไม่สำเร็จ
    pub fn end_object(&mut self) -> io::Result<()> {
        self.close(b"}")
    }

    /// เปิด array
    ///
    /// # Errors
    /// เมื่อเขียนไม่สำเร็จ
    pub fn begin_array(&mut self) -> io::Result<()> {
        self.out.write_all(b"[")?;
        self.empty.push(true);
        Ok(())
    }

    /// ปิด array
    ///
    /// # Errors
    /// เมื่อเขียนไม่สำเร็จ
    pub fn end_array(&mut self) -> io::Result<()> {
        self.close(b"]")
    }

    fn close(&mut self, bracket: &[u8]) -> io::Result<()> {
        let was_empty = self.empty.pop().unwrap_or(true);
        if !was_empty {
            self.out.write_all(b"\n")?;
            for _ in 0..self.empty.len() {
                self.out.write_all(b"  ")?;
            }
        }
        self.out.write_all(bracket)
    }

    /// ชื่อฟิลด์ใน object — ตามด้วยค่าเสมอ
    ///
    /// # Errors
    /// เมื่อเขียนไม่สำเร็จ
    pub fn key(&mut self, name: &str) -> io::Result<()> {
        self.sep()?;
        self.write_escaped(name)?;
        self.out.write_all(b": ")
    }

    /// สมาชิกตัวถัดไปของ array — ตามด้วยค่าเสมอ
    ///
    /// # Errors
    /// เมื่อเขียนไม่สำเร็จ
    pub fn next(&mut self) -> io::Result<()> {
        self.sep()
    }

    /// ค่าสตริง
    ///
    /// # Errors
    /// เมื่อเขียนไม่สำเร็จ
    pub fn string(&mut self, value: &str) -> io::Result<()> {
        self.write_escaped(value)
    }

    /// ค่าจำนวนเต็มไม่มีเครื่องหมาย
    ///
    /// # Errors
    /// เมื่อเขียนไม่สำเร็จ
    pub fn u64(&mut self, value: u64) -> io::Result<()> {
        write!(self.out, "{value}")
    }

    /// ค่าจำนวนเต็มมีเครื่องหมาย
    ///
    /// # Errors
    /// เมื่อเขียนไม่สำเร็จ
    pub fn i64(&mut self, value: i64) -> io::Result<()> {
        write!(self.out, "{value}")
    }

    /// ค่าทศนิยม — **ค่าที่ไม่ finite ออกมาเป็นสตริง** (ดูหัวโมดูล)
    ///
    /// # Errors
    /// เมื่อเขียนไม่สำเร็จ
    pub fn f32(&mut self, value: f32) -> io::Result<()> {
        if value.is_finite() {
            write!(self.out, "{value:?}")
        } else if value.is_nan() {
            self.out.write_all(b"\"NaN\"")
        } else if value > 0.0 {
            self.out.write_all(b"\"Infinity\"")
        } else {
            self.out.write_all(b"\"-Infinity\"")
        }
    }

    /// ค่าบูลีน
    ///
    /// # Errors
    /// เมื่อเขียนไม่สำเร็จ
    pub fn bool(&mut self, value: bool) -> io::Result<()> {
        self.out.write_all(if value { b"true" } else { b"false" })
    }

    /// `null`
    ///
    /// # Errors
    /// เมื่อเขียนไม่สำเร็จ
    pub fn null(&mut self) -> io::Result<()> {
        self.out.write_all(b"null")
    }

    /// ฟิลด์ + ค่าสตริง
    ///
    /// # Errors
    /// เมื่อเขียนไม่สำเร็จ
    pub fn field_str(&mut self, name: &str, value: &str) -> io::Result<()> {
        self.key(name)?;
        self.string(value)
    }

    /// ฟิลด์ + ค่าสตริงที่อาจไม่มี (`None` → `null`)
    ///
    /// # Errors
    /// เมื่อเขียนไม่สำเร็จ
    pub fn field_opt_str(&mut self, name: &str, value: Option<&str>) -> io::Result<()> {
        self.key(name)?;
        match value {
            Some(text) => self.string(text),
            None => self.null(),
        }
    }

    /// ฟิลด์ + จำนวนเต็ม
    ///
    /// # Errors
    /// เมื่อเขียนไม่สำเร็จ
    pub fn field_u64(&mut self, name: &str, value: u64) -> io::Result<()> {
        self.key(name)?;
        self.u64(value)
    }

    /// ฟิลด์ + จำนวนเต็มที่อาจไม่มี
    ///
    /// # Errors
    /// เมื่อเขียนไม่สำเร็จ
    pub fn field_opt_u64(&mut self, name: &str, value: Option<u64>) -> io::Result<()> {
        self.key(name)?;
        match value {
            Some(number) => self.u64(number),
            None => self.null(),
        }
    }

    /// ฟิลด์ + บูลีน
    ///
    /// # Errors
    /// เมื่อเขียนไม่สำเร็จ
    pub fn field_bool(&mut self, name: &str, value: bool) -> io::Result<()> {
        self.key(name)?;
        self.bool(value)
    }

    /// ฟิลด์ + บูลีนที่อาจไม่มี
    ///
    /// # Errors
    /// เมื่อเขียนไม่สำเร็จ
    pub fn field_opt_bool(&mut self, name: &str, value: Option<bool>) -> io::Result<()> {
        self.key(name)?;
        match value {
            Some(flag) => self.bool(flag),
            None => self.null(),
        }
    }

    /// ฟิลด์ + array ของ `f32`
    ///
    /// # Errors
    /// เมื่อเขียนไม่สำเร็จ
    pub fn field_f32s(&mut self, name: &str, values: &[f32]) -> io::Result<()> {
        self.key(name)?;
        self.begin_array()?;
        for value in values {
            self.next()?;
            self.f32(*value)?;
        }
        self.end_array()
    }

    /// escape ตามข้อกำหนดของ JSON
    fn write_escaped(&mut self, text: &str) -> io::Result<()> {
        self.out.write_all(b"\"")?;
        let mut plain = String::new();
        for ch in text.chars() {
            let escaped = match ch {
                '"' => "\\\"",
                '\\' => "\\\\",
                '\n' => "\\n",
                '\r' => "\\r",
                '\t' => "\\t",
                '\u{8}' => "\\b",
                '\u{c}' => "\\f",
                // อักขระควบคุมที่เหลือไม่มีชื่อย่อ ต้องเขียนเป็น \u00XX
                c if (c as u32) < 0x20 => {
                    self.out.write_all(plain.as_bytes())?;
                    plain.clear();
                    write!(self.out, "\\u{:04x}", c as u32)?;
                    continue;
                }
                c => {
                    plain.push(c);
                    continue;
                }
            };
            self.out.write_all(plain.as_bytes())?;
            plain.clear();
            self.out.write_all(escaped.as_bytes())?;
        }
        self.out.write_all(plain.as_bytes())?;
        self.out.write_all(b"\"")
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;

    fn render(build: impl FnOnce(&mut JsonWriter<Vec<u8>>)) -> String {
        let mut writer = JsonWriter::new(Vec::new());
        build(&mut writer);
        String::from_utf8(writer.finish().unwrap()).unwrap()
    }

    #[test]
    fn an_object_gets_commas_and_indentation() {
        let text = render(|w| {
            w.begin_object().unwrap();
            w.field_str("name", "a").unwrap();
            w.field_u64("n", 2).unwrap();
            w.key("list").unwrap();
            w.begin_array().unwrap();
            w.next().unwrap();
            w.u64(1).unwrap();
            w.next().unwrap();
            w.u64(2).unwrap();
            w.end_array().unwrap();
            w.end_object().unwrap();
        });
        assert_eq!(
            text,
            "{\n  \"name\": \"a\",\n  \"n\": 2,\n  \"list\": [\n    1,\n    2\n  ]\n}\n"
        );
    }

    #[test]
    fn empty_containers_stay_on_one_line() {
        let text = render(|w| {
            w.begin_object().unwrap();
            w.key("items").unwrap();
            w.begin_array().unwrap();
            w.end_array().unwrap();
            w.end_object().unwrap();
        });
        assert_eq!(text, "{\n  \"items\": []\n}\n");
    }

    /// ★★ สตริงมาจากไฟล์ของผู้ใช้ — escape ไม่ครบ = JSON ที่อ่านไม่ออก
    #[test]
    fn strings_from_a_file_cannot_break_out_of_their_quotes() {
        let text = render(|w| {
            w.string("a\"b\\c\nd\te\u{1}f\u{8}g").unwrap();
        });
        assert_eq!(text, "\"a\\\"b\\\\c\\nd\\te\\u0001f\\bg\"\n");
    }

    /// ★ ข้อความภาษาไทยต้องผ่านไปตรง ๆ ไม่ถูกแปลง — dump ต้องอ่านได้ด้วยตา
    #[test]
    fn non_ascii_text_is_written_as_is() {
        let text = render(|w| w.string("ภาพอ้างอิง 参考").unwrap());
        assert_eq!(text, "\"ภาพอ้างอิง 参考\"\n");
    }

    /// ★★★ **JSON ไม่มี `NaN`** — และค่านั้นมาถึงเราได้จริงเพราะไฟล์โกหกได้
    ///
    /// ถ้าพ่นออกไปดิบ ๆ ไฟล์ dump ทั้งก้อนจะ parse ไม่ผ่าน แปลว่าเครื่องมือที่
    /// มีไว้ debug ไฟล์พัง กลายเป็นพังเองเมื่อเจอไฟล์ที่พังพอดี
    #[test]
    fn values_json_cannot_express_become_strings() {
        let text = render(|w| {
            w.begin_array().unwrap();
            for value in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY, 1.5, -0.0] {
                w.next().unwrap();
                w.f32(value).unwrap();
            }
            w.end_array().unwrap();
        });
        assert_eq!(
            text,
            "[\n  \"NaN\",\n  \"Infinity\",\n  \"-Infinity\",\n  1.5,\n  -0.0\n]\n"
        );
    }
}
