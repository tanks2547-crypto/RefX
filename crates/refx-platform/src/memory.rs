//! ถาม OS ว่าเครื่องนี้มี RAM เท่าไหร่
//!
//! ใช้ตั้งเพดาน `max_pixels` ให้ผูกกับเครื่องจริง ไม่ใช่ค่าคงที่ (docs/05 §3)
//! เพราะภาพ 16384² ขอ RAM ~2 GiB ตอน decode ซึ่งบนเครื่อง 8 GB ที่เปิด Photoshop อยู่
//! = swap หนักหรือโดน OOM killer = **งานผู้ใช้หาย** ซึ่งผิด I-3
//!
//! อยู่ใน `refx-platform` เพราะเป็น crate เดียวที่อนุญาต `unsafe` (I-5)
//! และเป็นที่รวมโค้ดที่ขึ้นกับ OS (ADR-007)
//!
//! **ไม่เพิ่ม dependency ใหม่** — ประกาศ API ของ OS เองตรงนี้เลย
//! (`sysinfo` ไม่มีใน docs/09-crate-versions.md)

/// ค่าที่ใช้เมื่อถาม OS ไม่ได้ — เลือกต่ำไว้ก่อนเพื่อความปลอดภัย
///
/// เดาสูงเกินจริงแล้วโดน OOM = งานหาย ส่วนเดาต่ำแค่ทำให้เปิดภาพยักษ์ไม่ได้
/// ซึ่งแจ้งผู้ใช้ได้และไม่มีอะไรเสียหาย — ลำดับความสำคัญข้อ 1 มาก่อนข้อ 4
pub const FALLBACK_TOTAL_RAM: u64 = 8 << 30; // 8 GB

/// RAM ที่ติดตั้งในเครื่อง (ไบต์)
///
/// คืน [`FALLBACK_TOTAL_RAM`] ถ้าถาม OS ไม่ได้
#[must_use]
pub fn total_ram() -> u64 {
    platform_total_ram().unwrap_or_else(|| {
        tracing::warn!(
            fallback_gb = FALLBACK_TOTAL_RAM / (1 << 30),
            "ถามขนาด RAM จากระบบไม่ได้ — ใช้ค่าสำรองแบบระมัดระวัง"
        );
        FALLBACK_TOTAL_RAM
    })
}

#[cfg(target_os = "windows")]
fn platform_total_ram() -> Option<u64> {
    /// เลย์เอาต์ตรงกับ `MEMORYSTATUSEX` ของ Win32
    ///
    /// ประกาศเองเพื่อไม่ต้องเพิ่ม dependency — ลำดับและชนิดของฟิลด์
    /// ต้องตรงกับเอกสารของ Microsoft เป๊ะ ห้ามสลับ
    #[repr(C)]
    struct MemoryStatusEx {
        length: u32,
        memory_load: u32,
        total_phys: u64,
        avail_phys: u64,
        total_page_file: u64,
        avail_page_file: u64,
        total_virtual: u64,
        avail_virtual: u64,
        avail_extended_virtual: u64,
    }

    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GlobalMemoryStatusEx(buffer: *mut MemoryStatusEx) -> i32;
    }

    // SAFETY: `status` เป็นตัวแปรบนสแตกที่มีเลย์เอาต์ตรงกับ MEMORYSTATUSEX
    // และเราตั้ง `length` ให้เท่ากับขนาดจริงของ struct ตามที่ API บังคับก่อนเรียก
    // ตัวชี้ที่ส่งไปชี้ไปยัง storage ที่ยังมีชีวิตอยู่ตลอดการเรียก และ API
    // เขียนลงไปในนั้นอย่างเดียว ไม่เก็บตัวชี้ไว้ใช้ต่อ
    unsafe {
        let mut status = MemoryStatusEx {
            length: u32::try_from(size_of::<MemoryStatusEx>()).ok()?,
            memory_load: 0,
            total_phys: 0,
            avail_phys: 0,
            total_page_file: 0,
            avail_page_file: 0,
            total_virtual: 0,
            avail_virtual: 0,
            avail_extended_virtual: 0,
        };
        if GlobalMemoryStatusEx(&raw mut status) == 0 {
            return None; // API ล้มเหลว — ให้ผู้เรียกใช้ค่าสำรอง
        }
        (status.total_phys > 0).then_some(status.total_phys)
    }
}

#[cfg(target_os = "linux")]
fn platform_total_ram() -> Option<u64> {
    // /proc/meminfo อ่านได้ด้วย fs ธรรมดา ไม่ต้องใช้ unsafe เลย
    // รูปแบบ: "MemTotal:       16070024 kB"
    let text = std::fs::read_to_string("/proc/meminfo").ok()?;
    let line = text.lines().find(|l| l.starts_with("MemTotal:"))?;
    let kb: u64 = line.split_whitespace().nth(1)?.parse().ok()?;
    kb.checked_mul(1024)
}

#[cfg(not(any(target_os = "windows", target_os = "linux")))]
fn platform_total_ram() -> Option<u64> {
    // macOS ทำใน P6 — ตอนนี้ใช้ค่าสำรอง
    None
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::*;

    /// ค่าที่ได้ต้องสมเหตุสมผล ไม่ใช่ 0 หรือเลขบ้า ๆ
    ///
    /// ถ้าเลย์เอาต์ของ `MEMORYSTATUSEX` ผิด ค่าจะเพี้ยนไปคนละโลก แล้วเทสต์นี้จับได้
    #[test]
    fn total_ram_is_plausible() {
        let ram = total_ram();
        assert!(
            ram >= (1 << 30),
            "RAM ที่อ่านได้น้อยกว่า 1 GB ({ram} ไบต์) — น่าจะอ่านผิด"
        );
        assert!(
            ram <= (4096u64 << 30),
            "RAM ที่อ่านได้มากกว่า 4 TB ({ram} ไบต์) — น่าจะอ่านผิด"
        );
    }

    #[test]
    fn total_ram_is_stable() {
        // RAM ที่ติดตั้งไม่เปลี่ยนระหว่างโปรแกรมทำงาน
        assert_eq!(total_ram(), total_ram());
    }
}
