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
            "cannot ask the OS for total RAM — using a conservative fallback"
        );
        FALLBACK_TOTAL_RAM
    })
}

/// RAM ที่**โปรเซสนี้**ใช้อยู่จริง ณ ตอนนี้ และจุดสูงสุดตั้งแต่เปิดมา (ไบต์)
///
/// ★★★ **ทำไมต้องมี** — `docs/07 §6` บังคับว่าเพดาน RAM ตอน export ต้องพิสูจน์ด้วย
/// **RSS จริง ไม่ใช่ผลรวมบนกระดาษ** · การบวกขนาดบัฟเฟอร์ที่เราตั้งใจจองเข้าด้วยกัน
/// ตอบได้แค่ว่า *เราตั้งใจใช้เท่าไหร่* ไม่ได้ตอบว่า wgpu/ตัวเข้ารหัสจองอะไรไว้ข้างหลัง
/// — และของที่เรามองไม่เห็นคือของที่ทำให้เพดานพัง
///
/// ★ `peak` เป็นค่าที่ **ไม่ลดลง** ตลอดอายุโปรเซส (working set สูงสุดที่ OS เคยเห็น)
/// จึงใช้วัด "ยอดดอย" ของงานหนึ่งได้เฉพาะเมื่อ**อ่านก่อน–หลัง แล้วดูส่วนต่าง**
/// ไม่ใช่อ่านค่าเดียวแล้วเชื่อ
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProcessMemory {
    /// working set ปัจจุบัน (ไบต์) — **ส่วนที่ OS ยอมให้อยู่ใน RAM ตอนนี้**
    ///
    /// ★ OS ตัดทิ้งได้ตลอดเวลาเมื่อเครื่องหน่วยความจำตึง · ใช้ดูได้ ห้ามใช้ assert
    ///   เพดาน — ดู [`ProcessMemory::private`]
    pub current: u64,
    /// working set สูงสุดตั้งแต่โปรเซสเริ่ม (ไบต์)
    pub peak: u64,
    /// ★★★ หน่วยความจำส่วนตัวที่โปรเซส **ถือไว้** (ไบต์) — OS ตัดทิ้งไม่ได้
    ///
    /// | OS | อ่านจาก | นับอะไร |
    /// |---|---|---|
    /// | Windows | `PagefileUsage` (commit charge ของโปรเซส) | ทุกหน้าที่ commit แล้ว ไม่ว่าจะแตะหรือยัง |
    /// | Linux | `RssAnon + VmSwap` | หน้า anonymous ที่แตะแล้ว ไม่ว่าจะอยู่ใน RAM หรือถูกย้ายไป swap |
    ///
    /// สองแบบนี้ไม่เหมือนกันทุกประการ (Windows นับหน้าที่ commit แต่ยังไม่แตะด้วย)
    /// แต่มีคุณสมบัติเดียวที่เทสต์งบต้องการ: **เครื่องหน่วยความจำตึงแล้วค่าไม่หาย**
    ///
    /// ทำไมถึงเพิ่มเข้ามา: ดู `docs/07 §6` — `export_memory` แดง 2 ใน 5 รอบ
    /// เพราะ working set ถูก OS ตัดระหว่างรันเทสต์ขนานกัน (25 ก.ย. 2026)
    pub private: u64,
}

/// ถาม OS ว่าโปรเซสนี้ใช้ RAM ไปเท่าไหร่
///
/// `None` = แพลตฟอร์มนี้ยังตอบไม่ได้ (macOS ทำใน P6) หรือ API ล้ม
/// — ★ ผู้เรียกต้อง**พิมพ์ว่าข้าม** ห้ามเงียบแล้วรายงานว่าผ่าน (`docs/08 §3.9` ข้อ 2)
#[must_use]
pub fn process_memory() -> Option<ProcessMemory> {
    platform_process_memory()
}

#[cfg(target_os = "windows")]
fn platform_process_memory() -> Option<ProcessMemory> {
    /// เลย์เอาต์ตรงกับ `PROCESS_MEMORY_COUNTERS` ของ Win32
    ///
    /// บน 64 บิต `cb` + `page_fault_count` (DWORD คู่หนึ่ง) เต็ม 8 ไบต์พอดี
    /// ตัวถัดไปเป็น `SIZE_T` ซึ่ง align 8 อยู่แล้ว → ไม่มี padding แทรก
    /// **ห้ามสลับลำดับฟิลด์** ค่าที่อ่านได้จะเพี้ยนไปคนละโลกโดยไม่มี error
    #[repr(C)]
    struct ProcessMemoryCounters {
        cb: u32,
        page_fault_count: u32,
        peak_working_set_size: usize,
        working_set_size: usize,
        quota_peak_paged_pool: usize,
        quota_paged_pool: usize,
        quota_peak_non_paged_pool: usize,
        quota_non_paged_pool: usize,
        pagefile_usage: usize,
        peak_pagefile_usage: usize,
    }

    // ★ ใช้ `K32GetProcessMemoryInfo` ของ kernel32 ไม่ใช่ตัวใน psapi.dll —
    //   ตัวนี้อยู่ใน kernel32 ตั้งแต่ Windows 7 จึงไม่ต้องเพิ่มไลบรารีที่ต้องลิงก์
    //   (เหตุผลเดียวกับที่โมดูลนี้ประกาศ API เอง: ไม่เพิ่ม dependency)
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GetCurrentProcess() -> isize;
        fn K32GetProcessMemoryInfo(
            process: isize,
            counters: *mut ProcessMemoryCounters,
            cb: u32,
        ) -> i32;
    }

    // SAFETY: `counters` เป็นตัวแปรบนสแตกที่มีเลย์เอาต์ตรงกับ PROCESS_MEMORY_COUNTERS
    // และเราตั้ง `cb` ให้เท่ากับขนาดจริงของ struct ตามที่ API บังคับก่อนเรียก
    // `GetCurrentProcess` คืน pseudo-handle ที่ไม่ต้องปิด · API เขียนลงบัฟเฟอร์
    // อย่างเดียว ไม่เก็บตัวชี้ไว้ใช้ต่อ และเราตรวจค่าที่คืนทันที
    unsafe {
        let mut counters = ProcessMemoryCounters {
            cb: u32::try_from(size_of::<ProcessMemoryCounters>()).ok()?,
            page_fault_count: 0,
            peak_working_set_size: 0,
            working_set_size: 0,
            quota_peak_paged_pool: 0,
            quota_paged_pool: 0,
            quota_peak_non_paged_pool: 0,
            quota_non_paged_pool: 0,
            pagefile_usage: 0,
            peak_pagefile_usage: 0,
        };
        let cb = counters.cb;
        if K32GetProcessMemoryInfo(GetCurrentProcess(), &raw mut counters, cb) == 0 {
            return None;
        }
        Some(ProcessMemory {
            current: counters.working_set_size as u64,
            peak: counters.peak_working_set_size as u64,
            // ใน `PROCESS_MEMORY_COUNTERS` ช่องนี้คือ commit charge ของโปรเซส
            // (ค่าเดียวกับ `PrivateUsage` ของรุ่น `_EX`) — ชื่อเก่าหลงเหลือจากยุคที่
            // commit ทั้งหมดต้องมี pagefile รองรับ
            private: counters.pagefile_usage as u64,
        })
    }
}

#[cfg(target_os = "linux")]
fn platform_process_memory() -> Option<ProcessMemory> {
    // /proc/self/status: "VmRSS:     123456 kB" และ "VmHWM:     234567 kB"
    // ★ ข้อยกเว้นของกฎห้าม `fs::read_to_string` ด้วยเหตุผลเดียวกับ `platform_total_ram`
    //   ข้างล่าง — /proc เป็นไฟล์เสมือนของเคอร์เนล ไม่แตะดิสก์
    #[expect(
        clippy::disallowed_methods,
        reason = "/proc เป็นไฟล์เสมือน ไม่ใช่ดิสก์ I/O — เหตุผลเดียวกับ platform_total_ram"
    )]
    let text = std::fs::read_to_string("/proc/self/status").ok()?;
    let field = |name: &str| -> Option<u64> {
        let line = text.lines().find(|l| l.starts_with(name))?;
        let kb: u64 = line.split_whitespace().nth(1)?.parse().ok()?;
        kb.checked_mul(1024)
    };
    Some(ProcessMemory {
        current: field("VmRSS:")?,
        peak: field("VmHWM:")?,
        // ★ ไม่ใช้ `VmData` แม้ชื่อจะใกล้ "commit" กว่า — มันนับพื้นที่ที่แค่จองไว้
        //   (arena ของ allocator, stack ของเธรด) ซึ่งโตโดยไม่มีหน้าไหนถูกใช้จริง
        private: field("RssAnon:")?.checked_add(field("VmSwap:")?)?,
    })
}

#[cfg(not(any(target_os = "windows", target_os = "linux")))]
fn platform_process_memory() -> Option<ProcessMemory> {
    None // macOS ทำใน P6
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
    //
    // ★ ข้อยกเว้นของกฎห้ามใช้ `fs::read_to_string` (clippy.toml) — กฎนั้นมีไว้กัน
    //   **ดิสก์ I/O บน UI thread** (I-2) ซึ่งไม่ตรงกับที่นี่ด้วยสองเหตุผล:
    //     1. `/proc` เป็นไฟล์เสมือนของเคอร์เนล ไม่แตะดิสก์เลย ไม่มี seek ไม่มี
    //        network mount ไม่มีทางค้างแบบไฟล์บน OneDrive/NAS ที่กฎนั้นกันอยู่
    //     2. เรียกครั้งเดียวตอน init (`Limits::for_system`) ก่อนเข้าลูปเฟรม
    //        — ข้อยกเว้นแบบเดียวกับที่ `block_on` ใช้ได้เฉพาะตอน init (docs/09)
    //   ฝั่ง Windows ถามค่าเดียวกันผ่าน syscall จึงไม่ติดกฎนี้ตั้งแต่แรก
    #[expect(
        clippy::disallowed_methods,
        reason = "/proc เป็นไฟล์เสมือน + เรียกครั้งเดียวตอน init ไม่ใช่ดิสก์ I/O บนลูปเฟรม"
    )]
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

    // ---------- ★ มาตรวัด RSS ที่ `docs/07 §6` ใช้พิสูจน์เพดาน export ----------

    #[test]
    fn process_memory_is_plausible() {
        let Some(mem) = process_memory() else {
            println!("ข้าม: แพลตฟอร์มนี้ยังตอบ RSS ไม่ได้");
            return;
        };
        println!(
            "RSS ตอนนี้ {} MB · สูงสุด {} MB · private {} MB",
            mem.current >> 20,
            mem.peak >> 20,
            mem.private >> 20
        );
        assert!(mem.current > 0, "RSS เป็นศูนย์ — อ่านผิดแน่นอน");
        assert!(
            mem.private > 0 && mem.private < total_ram(),
            "private = {} ไบต์ — ศูนย์หรือเกิน RAM ทั้งเครื่อง แปลว่าอ่านผิดช่อง",
            mem.private
        );
        assert!(
            mem.peak >= mem.current,
            "ยอดสูงสุด ({}) ต่ำกว่าค่าปัจจุบัน ({}) — เลย์เอาต์ของ struct น่าจะสลับฟิลด์",
            mem.peak,
            mem.current
        );
        assert!(
            mem.current < total_ram(),
            "โปรเซสเดียวใช้ RAM มากกว่าที่เครื่องมี — อ่านผิด"
        );
    }

    /// ★★★ **มาตรวัดต้องพิสูจน์ว่ามันขยับจริง** (`docs/08 §3.9` ข้อ 9)
    ///
    /// เครื่องมือที่ผลิตหลักฐานต้องพิสูจน์ก่อนว่าตัวมันเองไม่โกหก · มาตรวัดที่คืน
    /// ค่าคงที่จะทำให้ทุกการวัดเพดาน export "ผ่าน" ตลอดกาล **โดยไม่ได้วัดอะไรเลย**
    /// — และเราจะรู้ตัววันที่ผู้ใช้ export แล้วเครื่องหมดแรม ซึ่งสายไปแล้ว
    #[test]
    fn the_rss_meter_actually_moves_when_memory_is_used() {
        let Some(before) = process_memory() else {
            println!("ข้าม: แพลตฟอร์มนี้ยังตอบ RSS ไม่ได้");
            return;
        };

        const BLOCK: usize = 64 << 20;
        // ★ ต้อง **แตะทุกหน้า** ไม่ใช่แค่จอง — หน่วยความจำที่จองแล้วไม่แตะยังไม่เข้า
        //   working set บนทั้งสอง OS การเขียนทุก 4 KB คือสิ่งที่ทำให้มันเข้าจริง
        let mut hog = vec![0u8; BLOCK];
        for page in hog.chunks_mut(4096) {
            page[0] = 1;
        }

        let during = process_memory().expect("อ่านได้ครั้งแรกแล้วต้องอ่านได้อีก");
        let growth = during.current.saturating_sub(before.current);
        println!(
            "จอง {} MB แล้วแตะทุกหน้า → RSS +{} MB",
            BLOCK >> 20,
            growth >> 20
        );

        // เผื่อไว้ครึ่งหนึ่ง: OS ตัด working set ระหว่างทางได้ แต่ถ้ามาตรวัดตายสนิท
        // ค่าจะเป็น 0 ซึ่งข้อนี้จับได้แน่นอน
        assert!(
            growth >= (BLOCK as u64) / 2,
            "จอง {} MB แล้ว RSS ขยับแค่ {} ไบต์ — มาตรวัดไม่ทำงาน",
            BLOCK >> 20,
            growth
        );
        assert!(during.peak >= during.current);

        // กันไม่ให้ตัว optimizer ตัดบล็อกทิ้งก่อนถึงจุดวัด
        assert_eq!(hog[0], 1);
        drop(hog);
    }

    /// ★★★ **NC ของมาตร `private`: สั่งให้ OS ตัด working set ของเราทิ้งเดี๋ยวนี้**
    /// แล้วดูว่ามาตรตัวไหนหาย ตัวไหนอยู่ (25 ก.ย. 2026)
    ///
    /// `export_memory` แดง 2 ใน 5 รอบของการรันทั้งชุด — มาตร working set อ่านได้
    /// +4 MB แล้วก็ +22 MB จากการจองและแตะ 64 MB (ปกติ +63) · สมมติฐานคือ Windows
    /// ตัด working set ทิ้งตอนเครื่องหน่วยความจำตึง
    ///
    /// รันทั้งชุดซ้ำหกรอบได้ `private` +64 ทุกรอบ ขณะที่ working set แกว่ง +63–+90
    /// **แต่ไม่มีรอบไหนเกิดการตัดจริง** · รอให้บังเอิญเกิดคือการพิสูจน์ด้วยโชค
    /// → เทสต์นี้ **สร้างเหตุการณ์นั้นเอง** ด้วย `SetProcessWorkingSetSize(-1, -1)`
    ///   (ทางที่ Microsoft ระบุไว้ให้โปรเซสขอให้ตัว working set ของตัวเอง) ·
    ///   แตะแค่โปรเซสนี้ ไม่ต้องกดดันหน่วยความจำของทั้งเครื่อง
    ///
    /// ต้องได้ทั้งสองข้อ — ข้อเดียวพิสูจน์อะไรไม่ได้:
    /// 1. working set **ลดจริง** — ไม่งั้นเราไม่ได้สร้างเหตุการณ์ที่สงสัยเลย
    /// 2. `private` **ไม่ลด** — มาตรใหม่ไม่ถูกเหตุการณ์นั้นแตะ
    #[cfg(target_os = "windows")]
    #[test]
    fn trimming_the_working_set_moves_rss_but_not_private_memory() {
        #[link(name = "kernel32")]
        unsafe extern "system" {
            fn GetCurrentProcess() -> isize;
            fn SetProcessWorkingSetSize(process: isize, min: usize, max: usize) -> i32;
        }

        const BLOCK: usize = 64 << 20;
        let mut hog = vec![0u8; BLOCK];
        for page in hog.chunks_mut(4096) {
            page[0] = 1;
        }
        let held = process_memory().expect("Windows ต้องตอบได้");

        // SAFETY: pseudo-handle ของโปรเซสตัวเองใช้ได้เสมอและไม่ต้องปิด · ค่า
        // (usize::MAX, usize::MAX) คือ (SIZE_T)-1 ทั้งคู่ ซึ่งเอกสารระบุว่าแปลว่า
        // "ตัด working set ทิ้งให้มากที่สุด" · API ไม่แตะหน่วยความจำของเรา แค่ย้าย
        // หน้าออกจาก working set · หน้ายังอยู่ครบ แตะอีกครั้งก็ถูกดึงกลับมา
        let ok = unsafe { SetProcessWorkingSetSize(GetCurrentProcess(), usize::MAX, usize::MAX) };
        assert_ne!(
            ok, 0,
            "SetProcessWorkingSetSize ล้ม — สร้างเหตุการณ์ที่ต้องการไม่ได้"
        );

        let trimmed = process_memory().expect("อ่านได้ครั้งแรกแล้วต้องอ่านได้อีก");
        println!(
            "ก่อนตัด: working set {} MB · private {} MB  →  หลังตัด: working set {} MB · private {} MB",
            held.current >> 20,
            held.private >> 20,
            trimmed.current >> 20,
            trimmed.private >> 20
        );

        assert!(
            trimmed.current + (BLOCK as u64) / 2 <= held.current,
            "working set ลดแค่ {} MB — เราไม่ได้สร้างเหตุการณ์ที่สงสัย การทดลองนี้จึงไม่พิสูจน์อะไร",
            held.current.saturating_sub(trimmed.current) >> 20
        );
        assert!(
            trimmed.private + (1 << 20) >= held.private,
            "private ลดจาก {} MB เหลือ {} MB ตอน OS ตัด working set — มาตรใหม่ก็ถูกตัดเหมือนกัน",
            held.private >> 20,
            trimmed.private >> 20
        );
        assert_eq!(hog[0], 1);
    }
}
