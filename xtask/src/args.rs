//! ตัวอ่านอาร์กิวเมนต์ของ xtask ที่ **ล้มเมื่อได้ของที่ไม่รู้จัก**
//!
//! # ทำไมต้องมีไฟล์นี้ (17 ก.ย. 2026)
//!
//! `ui-drive.ps1` อ่านแค่ `$parts[2]` แล้วทิ้งฟิลด์ที่สี่เงียบ ๆ → การทดลอง
//! device lost ทั้งรอบ **ไม่เคยเกิดขึ้น** แต่รายงานว่า "ผ่าน"
//! ([`docs/08 §3.9` ข้อ 9](../../docs/08-testing-and-budgets.md))
//!
//! ไล่ย้อนแล้วพบว่าเครื่องมือฝั่ง Rust เป็นชนชั้นเดียวกันทั้งแถบ:
//!
//! | ที่เดิม | สิ่งที่หายเงียบ ๆ |
//! |---|---|
//! | `icon.rs` · `licenses.rs` — `.any(\|a\| a == "--check")` | `--chek` → **สร้างไฟล์ใหม่แทนที่จะเป็นประตู** ใน CI คือเขียวที่ไม่ได้ตรวจอะไร |
//! | `mutation.rs` — `else { krate = arg }` | `mutation a b` เก็บแค่ `b` · ธงที่พิมพ์ผิดกลายเป็นชื่อ crate |
//! | `package.rs` — `args().nth(2)` | อาร์กิวเมนต์ที่เกินมาหายไปทั้งหมด |
//! | `main.rs` — คำสั่งที่ไม่รู้จัก | พิมพ์ข้อความแล้ว **คืน `Ok(())` = exit 0** |
//!
//! แถวสุดท้ายแย่ที่สุด: `cargo xtask licence --check` ใน CI จะ **เขียว**
//! ทั้งที่ไม่มีประตูไหนทำงานเลย
//!
//! # วิธีใช้
//!
//! อ่านตามลำดับ **ธง → ค่า → positional** แล้วปิดท้ายด้วย [`Args::finish`]
//! (ถ้าเรียก `positional()` ก่อน `value()` ตัว positional จะกินค่าของธงไป)
//!
//! ```ignore
//! let mut args = Args::new("cargo xtask icon [--check]");
//! let check = args.flag("--check");
//! args.finish()?;
//! ```

/// รายการอาร์กิวเมนต์ที่เหลืออยู่ · อะไรที่ยังเหลือตอน [`Args::finish`] คือ error
pub struct Args {
    items: Vec<String>,
    usage: &'static str,
}

impl Args {
    /// อ่านจาก `std::env::args()` โดยข้ามชื่อโปรแกรมและชื่อคำสั่งย่อย
    #[must_use]
    pub fn new(usage: &'static str) -> Self {
        Self::from_iter(std::env::args().skip(2), usage)
    }

    /// สำหรับเทสต์ — รับรายการตรง ๆ โดยไม่ผ่าน process
    pub fn from_iter<I: IntoIterator<Item = String>>(items: I, usage: &'static str) -> Self {
        Self {
            items: items.into_iter().collect(),
            usage,
        }
    }

    /// มีธงนี้ไหม (เอาออกจากรายการ) · ให้ซ้ำกี่ครั้งก็นับเป็นครั้งเดียว
    pub fn flag(&mut self, name: &str) -> bool {
        let before = self.items.len();
        self.items.retain(|a| a != name);
        before != self.items.len()
    }

    /// ค่าของธง รับได้ทั้ง `--name ค่า` และ `--name=ค่า`
    ///
    /// # Errors
    /// เมื่อมีธงแต่ไม่มีค่าตามมา · หรือให้ธงเดียวกันสองครั้ง
    pub fn value(&mut self, name: &str) -> anyhow::Result<Option<String>> {
        let eq = format!("{name}=");
        let usage = self.usage;
        let mut found: Option<String> = None;
        let mut rest = Vec::with_capacity(self.items.len());
        let mut it = self.items.drain(..);
        while let Some(arg) = it.next() {
            let got = if arg == name {
                Some(
                    it.next()
                        .ok_or_else(|| anyhow::anyhow!("{name} ต้องตามด้วยค่า\nใช้: {usage}"))?,
                )
            } else {
                arg.strip_prefix(&eq).map(ToOwned::to_owned)
            };
            match got {
                // ★ ให้ซ้ำสองครั้งคือความกำกวม ไม่ใช่ "ตัวหลังชนะ" — ถ้าเงียบ
                //   แล้วเลือกตัวใดตัวหนึ่ง คนสั่งจะเชื่อว่าได้อีกตัวตลอดไป
                Some(_) if found.is_some() => {
                    anyhow::bail!("ให้ {name} มาสองครั้ง — เอาอันที่ไม่ต้องการออก");
                }
                Some(v) => found = Some(v),
                None => rest.push(arg),
            }
        }
        drop(it);
        self.items = rest;
        Ok(found)
    }

    /// อาร์กิวเมนต์ตัวถัดไปที่ไม่ได้ขึ้นต้นด้วย `-`
    pub fn positional(&mut self) -> Option<String> {
        let at = self.items.iter().position(|a| !a.starts_with('-'))?;
        Some(self.items.remove(at))
    }

    /// ★★★ ประตู — **อะไรที่เหลืออยู่แปลว่าไม่มีใครอ่านมัน**
    ///
    /// # Errors
    /// เมื่อยังมีอาร์กิวเมนต์ที่ไม่มีใครรับไป
    pub fn finish(self) -> anyhow::Result<()> {
        anyhow::ensure!(
            self.items.is_empty(),
            "ไม่รู้จักอาร์กิวเมนต์: {}\n\
             มันจะถูกทิ้งเงียบ ๆ ซึ่งแปลว่าคำสั่งที่รันไปไม่ใช่คำสั่งที่สั่ง\n\
             ใช้: {}",
            self.items.join(" "),
            self.usage
        );
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::Args;

    fn args(list: &[&str]) -> Args {
        Args::from_iter(list.iter().map(|s| (*s).to_owned()), "ใช้: ...")
    }

    #[test]
    fn a_flag_that_is_there_is_seen_and_a_flag_that_is_not_is_not() {
        let mut a = args(&["--check"]);
        assert!(a.flag("--check"));
        assert!(!a.flag("--other"));
        assert!(a.finish().is_ok());
    }

    /// นี่คือบั๊กตัวจริงของ `icon.rs`/`licenses.rs`: พิมพ์ผิดหนึ่งตัว
    /// แล้วประตูกลายเป็นตัวสร้างไฟล์ โดยไม่มีใครรู้
    #[test]
    fn a_misspelled_flag_is_refused_instead_of_quietly_changing_what_runs() {
        let mut a = args(&["--chek"]);
        assert!(!a.flag("--check"));
        let err = a.finish().unwrap_err().to_string();
        assert!(err.contains("--chek"), "{err}");
    }

    #[test]
    fn a_value_reads_both_spellings() {
        let mut a = args(&["--out", "x.json"]);
        assert_eq!(a.value("--out").unwrap(), Some("x.json".to_owned()));
        assert!(a.finish().is_ok());

        let mut b = args(&["--out=y.json"]);
        assert_eq!(b.value("--out").unwrap(), Some("y.json".to_owned()));
        assert!(b.finish().is_ok());
    }

    #[test]
    fn a_value_flag_with_nothing_after_it_is_an_error() {
        let mut a = args(&["--out"]);
        assert!(a.value("--out").is_err());
    }

    #[test]
    fn the_same_value_twice_is_ambiguous_not_last_one_wins() {
        let mut a = args(&["--out=a", "--out=b"]);
        assert!(a.value("--out").is_err());
    }

    #[test]
    fn a_value_does_not_swallow_the_positional_next_to_it() {
        let mut a = args(&["file.refx", "--out", "x.json", "--verify-assets"]);
        assert_eq!(a.value("--out").unwrap(), Some("x.json".to_owned()));
        assert!(a.flag("--verify-assets"));
        assert_eq!(a.positional(), Some("file.refx".to_owned()));
        assert!(a.finish().is_ok());
    }

    /// `mutation a b` เคยเก็บแค่ `b` เงียบ ๆ
    #[test]
    fn a_second_positional_nobody_asked_for_is_refused() {
        let mut a = args(&["refx-core", "refx-io"]);
        assert_eq!(a.positional(), Some("refx-core".to_owned()));
        let err = a.finish().unwrap_err().to_string();
        assert!(err.contains("refx-io"), "{err}");
    }

    #[test]
    fn nothing_given_and_nothing_expected_is_fine() {
        assert!(args(&[]).finish().is_ok());
    }

    #[test]
    fn positionals_come_back_in_the_order_they_were_written() {
        let mut a = args(&["dir", "100", "4000", "3000"]);
        assert_eq!(a.positional().as_deref(), Some("dir"));
        assert_eq!(a.positional().as_deref(), Some("100"));
        assert_eq!(a.positional().as_deref(), Some("4000"));
        assert_eq!(a.positional().as_deref(), Some("3000"));
        assert_eq!(a.positional(), None);
        assert!(a.finish().is_ok());
    }
}
