//! ★★★ นาฬิกาของ "ผู้ใช้จำลอง" ในเทสต์ฆ่าโปรเซส — **วัด ไม่ใช่อนุมานจากเวลา**
//!
//! ## บทเรียนที่ทำให้ไฟล์นี้เกิด (CI แดง 17 ส.ค. 2026)
//!
//! เทสต์ฆ่าโปรเซสของ P4-3 ตอบคำถาม "ผู้ใช้เสียงานไปกี่วินาที" ด้วยการเทียบ
//! **จำนวน item ใน snapshot** กับ **จำนวนที่ผู้ใช้ทำไปแล้ว** — และรุ่นแรก
//! คำนวณตัวหลังจาก `เวลาที่มีชีวิต ÷ 20ms`
//!
//! สมมติฐานนั้นจริงบนเครื่องพัฒนา (6 core ว่าง ๆ) และ **ไม่จริงบน runner
//! 2 core ที่แชร์กับงานอื่น** ซึ่งแต่ละรอบกินเวลามากกว่า 20 ms จริง
//! ตัวหารจึงบอกว่าเหยื่อทำไป 163 ใบ ทั้งที่จริง ๆ ทำไปน้อยกว่านั้นมาก
//! → เทสต์รายงาน "เสีย 2.92s" แล้วแดง **ทั้งที่ autosave ทำงานถูกต้องทุกประการ**
//!
//! ```text
//! รอบ 7: มีชีวิต 3.267s · ทำไป ~163 ใบ · snapshot มี 17 ใบ · เสีย ~2.92s
//! ```
//!
//! นี่คือ `docs/08 §3.9` ข้อ 5b เป๊ะ ๆ — **การ assert นาฬิกาโดยอ้อม**
//! ผ่านตัวเลขที่ดูเหมือนนับของ · และมันคือชนิดของ flake ที่ข้อนั้นบอกว่า
//! "รับประกันว่าจะเกิดสักวัน" แล้วทำให้คนเริ่มไม่เชื่อสีแดง
//!
//! ## ทางที่ถูก
//!
//! เหยื่อ **จดความคืบหน้าของตัวเองลงไฟล์** ทุกครั้งที่ทำงานเสร็จหนึ่งชิ้น
//! พ่อจึงอ่านย้อนหลังได้ว่า *ใบที่ N ถูกสร้างตอนวินาทีที่เท่าไร* แทนที่จะหาร
//! เอาเอง — ตัวเลขที่ได้เป็นของจริงบนทุกเครื่อง เร็วหรือช้าก็ตาม
//!
//! ★ ไฟล์นี้คอมไพล์เฉพาะตอนเทสต์ ไม่มีอะไรจากที่นี่หลุดเข้า binary ของผู้ใช้

use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

/// ตัวจดของฝั่ง **เหยื่อ** — เรียก [`Self::record`] ทุกครั้งที่ "ผู้ใช้" ทำงานเสร็จหนึ่งชิ้น
pub(crate) struct Progress {
    file: std::fs::File,
    started: Instant,
}

impl Progress {
    /// เปิดไฟล์บันทึกความคืบหน้า
    pub(crate) fn new(path: &Path) -> std::io::Result<Self> {
        Ok(Self {
            file: std::fs::File::create(path)?,
            started: Instant::now(),
        })
    }

    /// จดว่า ณ ตอนนี้ผู้ใช้ทำไปแล้ว `items` ชิ้น
    ///
    /// ★ เขียนต่อท้ายบรรทัดละรายการ · ถูกฆ่ากลางบรรทัดได้ ซึ่ง [`read`] ตัดทิ้งเอง
    /// ★ `flush` ทุกครั้งเพราะถ้าค้างใน buffer แล้วโดนฆ่า ข้อมูลที่ใช้วัดจะหาย
    ///   ไปพร้อมกัน — ซึ่งจะทำให้เราวัด*น้อยกว่า*ความจริง (มองไม่เห็นความเสียหาย)
    pub(crate) fn record(&mut self, items: usize) {
        let ms = self.started.elapsed().as_millis();
        let _ = writeln!(self.file, "{items},{ms}");
        let _ = self.file.flush();
    }
}

/// ★ ประวัติที่อ่านกลับมาได้หลังเหยื่อตาย — `(จำนวนชิ้น, เวลาตั้งแต่เริ่ม)`
#[derive(Debug, Default)]
pub(crate) struct Timeline(Vec<(usize, Duration)>);

/// อ่านประวัติที่เหยื่อจดไว้ — บรรทัดที่พังถูกข้าม (ถูกฆ่ากลางเขียนเป็นเรื่องปกติ)
pub(crate) fn read(path: &Path) -> Timeline {
    use std::io::Read as _;

    let mut text = String::new();
    let Ok(mut file) = std::fs::File::open(path) else {
        return Timeline::default();
    };
    if file.read_to_string(&mut text).is_err() {
        return Timeline::default();
    }
    Timeline(
        text.lines()
            .filter_map(|line| {
                let (items, ms) = line.split_once(',')?;
                Some((
                    items.trim().parse().ok()?,
                    Duration::from_millis(ms.trim().parse().ok()?),
                ))
            })
            .collect(),
    )
}

impl Timeline {
    /// ผู้ใช้ทำไปทั้งหมดกี่ชิ้นก่อนถูกฆ่า — `None` = ไม่เคยจดอะไรเลย
    pub(crate) fn done(&self) -> Option<usize> {
        self.0.last().map(|&(items, _)| items)
    }

    /// ★★ **งานที่หายไปคิดเป็นเวลาเท่าไร** ถ้า snapshot เก็บได้ `saved` ชิ้น
    ///
    /// = เวลาของชิ้นสุดท้ายที่ทำ − เวลาของชิ้นสุดท้ายที่รอด
    /// · ทั้งสองค่าเป็น**เวลาที่เหยื่อจดไว้เอง** ไม่ใช่ค่าที่เราหารจากนาฬิกาของพ่อ
    ///
    /// `None` = วัดไม่ได้ (ไม่มีประวัติ หรือ snapshot ใหม่กว่าบรรทัดสุดท้ายที่จดทัน)
    pub(crate) fn lost_after(&self, saved: usize) -> Option<Duration> {
        let (_, last_at) = *self.0.last()?;
        // ชิ้นที่ `saved` ถูกสร้างเมื่อไหร่ — snapshot ถูกตรึงไว้ ณ จังหวะนั้น
        let saved_at = self
            .0
            .iter()
            .rev()
            .find_map(|&(items, at)| (items <= saved).then_some(at))?;
        Some(last_at.saturating_sub(saved_at))
    }
}

/// ที่อยู่ของไฟล์ประวัติที่คู่กับงานชิ้นนี้
pub(crate) fn progress_path(dir: &Path) -> PathBuf {
    dir.join("progress.csv")
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::*;

    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "refx-killclock-{tag}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// ★ เวลาที่หายไปต้องมาจาก **ประวัติที่จดไว้** ไม่ใช่จากการหาร
    #[test]
    fn lost_time_comes_from_what_the_victim_recorded() {
        let dir = temp_dir("basic");
        let path = progress_path(&dir);
        // จดเอง 5 บรรทัดโดยไม่ต้องรอเวลาจริง (นี่คือประเด็นทั้งหมดของไฟล์นี้)
        std::fs::write(&path, "1,0\n2,20\n3,45\n4,900\n5,1500\n").unwrap();

        let timeline = read(&path);
        assert_eq!(timeline.done(), Some(5));
        // snapshot เก็บได้ 3 ชิ้น → เสียเวลาตั้งแต่ชิ้นที่ 3 (45ms) ถึงชิ้นที่ 5 (1500ms)
        assert_eq!(
            timeline.lost_after(3),
            Some(Duration::from_millis(1455)),
            "คำนวณเวลาที่เสียผิด"
        );
        // เก็บได้ครบ = ไม่เสียอะไรเลย
        assert_eq!(timeline.lost_after(5), Some(Duration::ZERO));
    }

    /// ★★ ถูกฆ่ากลางบรรทัด = บรรทัดนั้นถูกทิ้ง ไม่ใช่ทำให้อ่านทั้งไฟล์ไม่ได้
    #[test]
    fn a_line_torn_by_the_kill_is_dropped_not_fatal() {
        let dir = temp_dir("torn");
        let path = progress_path(&dir);
        std::fs::write(&path, "1,0\n2,20\n3,4").unwrap(); // บรรทัดท้ายขาดกลาง

        let timeline = read(&path);
        assert_eq!(
            timeline.done(),
            Some(3),
            "บรรทัดที่ยังไม่มี newline แต่ parse ได้ ยังใช้ได้"
        );
        std::fs::write(&path, "1,0\n2,20\nขยะ").unwrap();
        assert_eq!(read(&path).done(), Some(2), "บรรทัดที่พังต้องถูกข้าม");
    }

    /// ไม่มีไฟล์ = วัดไม่ได้ ไม่ใช่ panic
    #[test]
    fn no_history_is_measurable_as_nothing() {
        let dir = temp_dir("empty");
        let timeline = read(&progress_path(&dir));
        assert_eq!(timeline.done(), None);
        assert_eq!(timeline.lost_after(1), None);
    }

    /// ตัวจดจริงต้องอ่านกลับได้ และเวลาต้องเดินหน้าเสมอ
    #[test]
    fn what_the_recorder_writes_reads_back() {
        let dir = temp_dir("roundtrip");
        let path = progress_path(&dir);
        let mut progress = Progress::new(&path).unwrap();
        for items in 1..=4 {
            progress.record(items);
            std::thread::sleep(Duration::from_millis(2));
        }
        let timeline = read(&path);
        assert_eq!(timeline.done(), Some(4));
        // ★ assert **ลำดับ** ไม่ใช่ค่าเวลา — ค่าเวลาเป็นของเครื่อง (§3.9 ข้อ 5b)
        let times: Vec<_> = timeline.0.iter().map(|&(_, at)| at).collect();
        assert!(times.windows(2).all(|w| w[0] <= w[1]), "เวลาเดินถอยหลัง");
    }
}
