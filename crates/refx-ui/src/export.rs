//! ต่อสายฝั่ง GPU เข้ากับฝั่งตัวเข้ารหัส — งาน export หนึ่งงานทั้งเส้น (P5-4)
//!
//! ★ crate นี้เป็นที่เดียวที่รู้จักทั้ง `refx-render` และ `refx-asset` จึงเป็นที่
//! เดียวที่ประกอบสองฝั่งเข้าด้วยกันได้ · รูปแบบเดียวกับที่ `refx-ui` เป็นคนส่ง
//! `rename_durable` ให้ `refx-io` (ดู `refx_io::save::RenameFn`)
//!
//! ## I-2 — ห้ามเรียกจาก UI thread
//!
//! [`export_board`] **บล็อกรอ GPU** ทุกแถบ และเข้ารหัสทั้งไฟล์ในเธรดที่เรียก
//! `docs/07 §6` ข้อ 1 บังคับให้ทำงานนี้อยู่บน worker · ผู้เรียกต้องยิงมันจาก
//! เธรดของตัวเองแล้วส่งผลกลับมาทางช่อง (จะทำพร้อม dialog ใน P5-4 รอบถัดไป)
//!
//! ## เพดาน RAM
//!
//! `docs/07 §6` บังคับว่าต้องพิสูจน์ด้วย **RSS จริง** ไม่ใช่ผลรวมบนกระดาษ —
//! ดูเทสต์ [`tests::what_an_export_really_costs_in_process_memory`]
//!
//! spec: docs/07-file-format.md §6

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};

use refx_asset::export::{BandError, BandSource, ExportFormat, ExportStats, RenameFn};
use refx_core::export::BandPlan;
use refx_core::geom::Rect;
use refx_render::export::BandRenderer;
use refx_render::instance::QuadInstance;
use refx_render::pipeline::DrawBatch;
use refx_render::texture::TextureAllocator;

/// สัญญาณสองทางระหว่าง UI กับ worker
///
/// ★ อยู่ด้วยกันเพราะมันเป็นของคู่กันเสมอ: ปุ่มยกเลิกจะมีความหมายก็ต่อเมื่อ
/// ผู้ใช้เห็นว่างานไปถึงไหนแล้ว · แยกกันเมื่อไหร่จะมีจุดเรียกที่ส่งมาแค่ตัวเดียว
#[derive(Clone, Copy)]
pub struct JobSignals<'a> {
    /// ตั้งเป็น `true` = ขอให้หยุด · อ่านใน **ตัวป้อนพิกเซล** ไม่ใช่แค่ระหว่างแถบ
    pub cancel: &'a AtomicBool,
    /// แถบที่เขียนเสร็จแล้ว — `None` = ไม่มีใครดู (เทสต์)
    pub progress: Option<&'a AtomicU32>,
    /// ★★ ตัวปลุก event loop — เรียกทุกครั้งที่ [`Self::progress`] ขยับ
    ///
    /// ถ้าไม่ปลุก แถบความคืบหน้าจะค้างนิ่งจนกว่าผู้ใช้จะขยับเมาส์ ซึ่งอ่านได้ว่า
    /// โปรแกรมแฮงก์ · **ตัวเลขที่อัปเดตแล้วไม่มีใครวาด เท่ากับไม่ได้อัปเดต**
    pub waker: Option<&'a refx_platform::window::Waker>,
}

impl<'a> JobSignals<'a> {
    /// สัญญาณที่มีแต่ธงยกเลิก
    #[must_use]
    pub fn just_cancel(cancel: &'a AtomicBool) -> Self {
        Self {
            cancel,
            progress: None,
            waker: None,
        }
    }
}

/// สิ่งที่ผู้ใช้เลือกในกล่อง export
#[derive(Debug, Clone, PartialEq)]
pub struct ExportRequest {
    /// ไฟล์ปลายทาง — ผู้เรียกต้องผ่าน `validate_asset_path()` มาก่อนแล้ว
    pub path: PathBuf,
    /// รูปแบบไฟล์
    pub format: ExportFormat,
    /// ขนาดภาพปลายทาง (พิกเซล)
    pub size: (u32, u32),
    /// กรอบใน world ที่จะถูก export (ปกติคือขอบเขตของทั้ง board)
    pub region: Rect,
    /// สีพื้นหลัง sRGB
    ///
    /// ★ JPEG ไม่มี alpha → ผู้เรียก**ต้อง**ส่ง alpha 255 มา ไม่งั้นภาพจะได้
    /// พื้นดำโดยไม่ได้ตั้งใจ (`docs/07 §6`) · [`export_board`] บังคับข้อนี้ให้
    pub background: [u8; 4],
}

/// export ไม่สำเร็จ
#[derive(Debug, thiserror::Error)]
pub enum ExportJobError {
    /// ★ ไม่มีอะไรให้วาด — **ปฏิเสธ ห้ามเขียนไฟล์ 0 ไบต์** (`docs/07 §6` ข้อ 5)
    #[error("there is nothing on this board to export")]
    NothingToExport,
    /// ที่ที่จะเขียนไม่ปลอดภัย — ดู [`refx_io::validate::validate_save_target`]
    #[error(transparent)]
    BadTarget(#[from] refx_io::validate::SecError),
    /// ระบบไม่มีเธรดให้แล้ว — ใหญ่กว่าเรื่อง export
    #[error("the system would not give us a thread to export on")]
    NoWorker,
    /// worker หายไปโดยไม่ส่งผลกลับมา (panic ระหว่างทาง)
    ///
    /// ★ ต้องมี variant นี้ ไม่งั้น UI จะรอผลที่ไม่มีวันมา **ตลอดกาล**
    /// แล้วปุ่มยกเลิกก็ไม่ช่วยอะไรเพราะไม่มีใครฟังธงแล้ว
    #[error("the export worker stopped without saying why — see the log")]
    Lost,
    /// ขนาดที่ขอ export ไม่ได้
    #[error(transparent)]
    Plan(#[from] refx_core::export::PlanError),
    /// เตรียมเป้าบน GPU ไม่ได้
    #[error(transparent)]
    Render(#[from] refx_render::export::ExportRenderError),
    /// เขียนไฟล์ไม่ได้ / ถูกยกเลิก
    #[error(transparent)]
    Write(#[from] refx_asset::export::ExportError),
}

/// ต้นทางพิกเซลที่วาดจาก GPU ทีละแถบ
///
/// ★ ทำหน้าที่เดียว: แปลง "ขอแถวที่ `y0`" เป็น "วาดแถบที่ `index`" ·
/// ตัวมันเอง**ไม่ถือบัฟเฟอร์แถบ** — เขียนลงบัฟเฟอร์ของตัวเข้ารหัสตรง ๆ
/// (เหตุผลเต็มอยู่ที่ `BandRenderer::render_band`)
struct GpuBands<'a> {
    renderer: BandRenderer,
    device: &'a wgpu::Device,
    queue: &'a wgpu::Queue,
    batches: &'a [DrawBatch<'a>],
    progress: Option<&'a AtomicU32>,
    waker: Option<&'a refx_platform::window::Waker>,
}

impl BandSource for GpuBands<'_> {
    fn fill(&mut self, y0: u32, rows: u32, out: &mut [u8]) -> Result<(), BandError> {
        let band_rows = self.renderer.plan().band_rows();
        // ★ ตัวเข้ารหัสขอเป็น "แถวที่เท่าไหร่" ส่วนเราวาดเป็น "แถบที่เท่าไหร่" —
        //   ถ้าสองอย่างนี้ไม่ตรงกันเป๊ะ ภาพจะเหลื่อมทีละแถบโดยไม่มี error
        //   จึงตรวจซ้ำแทนที่จะเชื่อ (ดู `docs/08 §3.9` ข้อ 8)
        if !y0.is_multiple_of(band_rows) {
            return Err(BandError(format!(
                "row {y0} is not the start of a band of {band_rows} rows"
            )));
        }
        let written = self
            .renderer
            .render_band(self.device, self.queue, y0 / band_rows, self.batches, out)
            .map_err(|err| BandError(err.to_string()))?;
        if written != rows {
            return Err(BandError(format!(
                "the renderer produced {written} rows where {rows} were asked for at row {y0}"
            )));
        }
        // ★ นับ **หลัง** วาดเสร็จ — ตัวเลขที่ผู้ใช้เห็นต้องหมายถึงงานที่ทำไปแล้วจริง
        if let Some(progress) = self.progress {
            progress.store(y0 / band_rows + 1, Ordering::Release);
            if let Some(waker) = self.waker {
                waker.wake();
            }
        }
        Ok(())
    }
}

/// จำนวน instance ทั้งหมดในทุกก้อน — ศูนย์ = ไม่มีอะไรให้ export
#[must_use]
pub fn instances_in(batches: &[DrawBatch<'_>]) -> usize {
    batches.iter().map(|batch| batch.instances.len()).sum()
}

/// ของบน GPU ที่ export ต้องใช้ — มัดไว้ก้อนเดียวเพราะทั้งสี่ตัวมาจากที่เดียวกัน
/// (`RenderContext` + atlas ของแอป) และเดินทางไปด้วยกันเสมอ
#[derive(Clone, Copy)]
pub struct GpuAccess<'a> {
    /// device ที่ทุกอย่างผูกอยู่
    pub device: &'a wgpu::Device,
    /// คิวคำสั่ง
    pub queue: &'a wgpu::Queue,
    /// ทางเดียวที่จอง texture ได้ (I-6)
    pub allocator: &'a TextureAllocator,
    /// layout ของ bind group ที่ shader ใช้อ่าน texture
    pub atlas_layout: &'a wgpu::BindGroupLayout,
}

/// เขียนไฟล์ export ของ board หนึ่งใบ — **ต้องเรียกจาก worker ไม่ใช่ UI thread**
///
/// # Errors
/// [`ExportJobError`] — board ว่าง, ขนาดเกินเพดาน, GPU ล้ม, ถูกยกเลิก
/// หรือระบบไฟล์ล้ม · **ไฟล์ปลายทางเดิม (ถ้ามี) ยังอยู่ครบเสมอ**
pub fn export_board(
    gpu: GpuAccess<'_>,
    batches: &[DrawBatch<'_>],
    request: &ExportRequest,
    signals: JobSignals<'_>,
    rename: RenameFn,
) -> Result<ExportStats, ExportJobError> {
    // ★★ ปฏิเสธ**ก่อน**แตะดิสก์ — ไฟล์ 0 ไบต์ที่ชื่อเหมือนงานของผู้ใช้
    //    อ่านได้ว่า "โปรแกรมทำงานหาย" ซึ่งแย่กว่าการบอกตรง ๆ ว่าไม่มีอะไรให้เขียน
    if instances_in(batches) == 0 {
        return Err(ExportJobError::NothingToExport);
    }

    // ★★★ **ด่านของ path ที่ผู้ใช้เลือกเอง — ไม่ใช่ `validate_asset_path()`**
    //
    // สเปกเดิมสั่งให้ใช้ด่านของ path ที่มาจากไฟล์ ซึ่งจะ **ปฏิเสธการ export ลง
    // ไดรฟ์ที่แชร์ไว้** — งานประจำของนักวาดที่ทำงานเป็นทีม · `docs/06 §4`
    // แก้แล้ว 8 ก.ย. 2026: อันตรายของ UNC ไม่ใช่ "มันคือเครือข่าย" แต่คือ
    // **"ใครเป็นคนเลือก path นั้น"** · ที่นี่คนเลือกคือผู้ใช้
    //
    // ★★ ด่านอยู่ **ในนี้** ไม่ใช่ในตัวเรียก — ถ้าอยู่ในตัวเรียก ทุกตัวเรียกใหม่
    //    ต้องจำให้ได้เอง แล้ววันหนึ่งจะมีตัวที่ลืม (`docs/08 §3.9` ข้อ 8)
    refx_io::validate::validate_save_target(&request.path)?;

    let plan = BandPlan::new(request.size.0, request.size.1)?;

    // ★★★ JPEG ไม่มี alpha — บังคับทึบที่นี่ **ไม่ใช่หวังว่าผู้เรียกจำได้**
    //     พลาดข้อนี้ = ผู้ใช้ได้ภาพพื้นดำโดยไม่ได้ตั้งใจ (`docs/07 §6`)
    let mut background = request.background;
    if matches!(request.format, ExportFormat::Jpeg { .. }) {
        background[3] = 255;
    }

    let renderer = BandRenderer::new(
        gpu.device,
        gpu.allocator,
        gpu.atlas_layout,
        plan,
        request.region,
        background,
    )?;
    let mut source = GpuBands {
        renderer,
        device: gpu.device,
        queue: gpu.queue,
        batches,
        progress: signals.progress,
        waker: signals.waker,
    };

    let stats = refx_asset::export::export_to_file(
        &request.path,
        request.format,
        plan,
        &mut source,
        signals.cancel,
        rename,
    )?;
    Ok(stats)
}

/// ก้อนที่ worker วาดได้ — **สำเนาที่ถือของเอง** ไม่ใช่ยืมจากสถานะของแอป
///
/// ★★★ handle ของ wgpu ทุกตัวเป็น `Arc` อยู่แล้ว การ clone จึงไม่ได้ก๊อป texture
/// สักไบต์ · สิ่งที่ก๊อปจริงคือ `Vec<QuadInstance>` (64 ไบต์ต่อใบ — 3,072 ใบ = 196 KB)
/// ซึ่งเป็น **ภาพนิ่งของ board ณ วินาทีที่กด export** · นั่นถูกต้องแล้ว: ผู้ใช้
/// ที่ลากภาพเพิ่มระหว่าง export คาดหวังไฟล์ที่ตรงกับตอนที่เขากดปุ่ม ไม่ใช่
/// ไฟล์ที่ครึ่งบนเป็นก่อนลากและครึ่งล่างเป็นหลังลาก
pub struct OwnedBatch {
    /// texture ที่ instance ชุดนี้ใช้
    pub bind_group: wgpu::BindGroup,
    /// instance ที่ใช้ texture นั้น
    pub instances: Vec<QuadInstance>,
}

/// handle ของ GPU ที่ worker ถือไปเอง — [`GpuAccess`] รุ่นที่ไม่ยืมใคร
///
/// ★ ทุกตัวเป็น `Arc` อยู่แล้ว การ clone จึงไม่ได้ก๊อป device หรือ texture
pub struct OwnedGpu {
    /// device ที่ทุกอย่างผูกอยู่
    pub device: wgpu::Device,
    /// คิวคำสั่ง
    pub queue: wgpu::Queue,
    /// ทางเดียวที่จอง texture ได้ (I-6)
    pub allocator: TextureAllocator,
    /// layout ของ bind group ที่ shader ใช้อ่าน texture
    pub atlas_layout: wgpu::BindGroupLayout,
}

/// งาน export ที่กำลังทำอยู่บน worker
///
/// ★ `docs/07 §6` ข้อ 1: งาน render + encode อยู่ที่ worker · UI ต้องลากหน้าต่าง
/// ได้ระหว่าง export · ★★ และ **ปุ่มยกเลิกต้องตอบสนองทันที** ซึ่งเป็นเหตุผล
/// ที่ธงถูกอ่านในตัวป้อนพิกเซล ไม่ใช่แค่ระหว่างแถบ
pub struct ExportJob {
    cancel: Arc<AtomicBool>,
    done: crossbeam_channel::Receiver<Result<ExportStats, ExportJobError>>,
    /// แถบที่เขียนไปแล้ว — worker เขียน UI อ่าน
    progress: Arc<AtomicU32>,
    bands: u32,
    started: std::time::Instant,
    /// ชื่อไฟล์ที่กำลังเขียน (ไว้ทำข้อความบอกผู้ใช้)
    name: String,
}

impl ExportJob {
    /// ยิงงาน export ลง worker แล้วคืนทันที
    ///
    /// ★ เธรดนี้ **ไม่ถูก join** ด้วยเหตุผลเดียวกับ dialog: ปิดโปรแกรมทั้งที่
    /// export ยังไม่จบ ต้องไม่ทำให้การปิดค้าง · `Receiver` ที่ถูก drop ทำให้
    /// `send` ฝั่งโน้นล้มเงียบ ๆ ซึ่งเป็นพฤติกรรมที่ต้องการพอดี
    ///
    /// # Errors
    /// [`ExportJobError`] เมื่อสร้างเธรดไม่ได้ หรือคำขอไม่ผ่านด่านตั้งแต่ต้น
    pub fn spawn(
        gpu: OwnedGpu,
        batches: Vec<OwnedBatch>,
        request: ExportRequest,
        rename: RenameFn,
        waker: Option<refx_platform::window::Waker>,
    ) -> Result<Self, ExportJobError> {
        let OwnedGpu {
            device,
            queue,
            allocator,
            atlas_layout,
        } = gpu;
        // ★★ ตรวจสิ่งที่ตอบได้ทันที **ก่อนสร้างเธรด** — ผู้ใช้ที่พิมพ์ชื่อไฟล์ผิด
        //    ต้องเห็น error ในเฟรมเดียวกับที่กด ไม่ใช่หลังจากหมุนไปครึ่งวินาที
        if batches.iter().map(|b| b.instances.len()).sum::<usize>() == 0 {
            return Err(ExportJobError::NothingToExport);
        }
        refx_io::validate::validate_save_target(&request.path)?;
        let bands = BandPlan::new(request.size.0, request.size.1)?.band_count();

        let cancel = Arc::new(AtomicBool::new(false));
        let progress = Arc::new(AtomicU32::new(0));
        let (tx, done) = crossbeam_channel::bounded(1);
        let name = request
            .path
            .file_name()
            .map_or_else(String::new, |n| n.to_string_lossy().into_owned());

        let worker = {
            let (cancel, progress) = (Arc::clone(&cancel), Arc::clone(&progress));
            let tick = waker.clone();
            std::thread::Builder::new()
                .name("refx-export".to_owned())
                .spawn(move || {
                    let borrowed: Vec<DrawBatch<'_>> = batches
                        .iter()
                        .map(|batch| DrawBatch {
                            bind_group: &batch.bind_group,
                            instances: &batch.instances,
                        })
                        .collect();
                    let result = export_board(
                        GpuAccess {
                            device: &device,
                            queue: &queue,
                            allocator: &allocator,
                            atlas_layout: &atlas_layout,
                        },
                        &borrowed,
                        &request,
                        JobSignals {
                            cancel: &cancel,
                            progress: Some(&progress),
                            waker: tick.as_ref(),
                        },
                        rename,
                    );
                    // ★ ปักหมุดว่า "จบแล้ว" ไม่ว่าจะสำเร็จหรือไม่ — แถบความคืบหน้า
                    //   ที่ค้างอยู่ที่ 30/32 หลังงานล้ม อ่านได้ว่าโปรแกรมค้าง
                    progress.store(u32::MAX, Ordering::Release);
                    // ★★★ ปลุก event loop ที่หลับอยู่ ไม่งั้นผู้ใช้จะไม่รู้ว่างานจบ
                    //     จนกว่าจะขยับเมาส์ (เงื่อนไขข้อ 2 ของ `docs/04 §1`)
                    if let Some(waker) = &waker {
                        waker.wake();
                    }
                    let _ = tx.send(result);
                })
        };
        if let Err(err) = worker {
            tracing::error!(%err, "cannot spawn the export worker");
            return Err(ExportJobError::NoWorker);
        }

        Ok(Self {
            cancel,
            done,
            progress,
            bands,
            started: std::time::Instant::now(),
            name,
        })
    }

    /// ขอให้หยุด — **ไม่บล็อก** · ผลจริงมาถึงทาง [`Self::finished`] เหมือนเดิม
    pub fn cancel(&self) {
        self.cancel.store(true, Ordering::Release);
    }

    /// ผู้ใช้กดยกเลิกไปแล้วหรือยัง (ไว้เปลี่ยนข้อความบนปุ่ม)
    #[must_use]
    pub fn cancelling(&self) -> bool {
        self.cancel.load(Ordering::Relaxed)
    }

    /// แถบที่เขียนไปแล้ว / ทั้งหมด
    #[must_use]
    pub fn progress(&self) -> (u32, u32) {
        (
            self.progress.load(Ordering::Acquire).min(self.bands),
            self.bands,
        )
    }

    /// ชื่อไฟล์ที่กำลังเขียน
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// เวลาที่ใช้ไปตั้งแต่เริ่ม
    #[must_use]
    pub fn elapsed(&self) -> std::time::Duration {
        self.started.elapsed()
    }

    /// ★ **ไม่บล็อก** — `None` = ยังทำอยู่
    ///
    /// เรียกได้ทุกเฟรมโดยไม่มีราคา · ห้ามใช้ `recv()` ที่นี่เด็ดขาด (I-2)
    pub fn finished(&self) -> Option<Result<ExportStats, ExportJobError>> {
        match self.done.try_recv() {
            Ok(result) => Some(result),
            // ★ เธรดตายโดยไม่ส่งอะไรมา (panic ระหว่างทาง) = ล้ม ไม่ใช่ค้างตลอดกาล
            Err(crossbeam_channel::TryRecvError::Disconnected) => Some(Err(ExportJobError::Lost)),
            Err(crossbeam_channel::TryRecvError::Empty) => None,
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use refx_core::glam::Vec2;
    use refx_render::atlas::ThumbnailAtlas;
    use refx_render::instance::{QuadInstance, pack_tint};

    use super::*;

    /// GPU + atlas ที่มีพิกเซลจริงอยู่หนึ่งช่อง
    fn scene() -> Option<(
        wgpu::Device,
        wgpu::Queue,
        TextureAllocator,
        ThumbnailAtlas,
        wgpu::Instance,
    )> {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
        // ★ ไม่มีการ์ดจริงก็ยังเทสต์ได้ — ขอ software adapter แทน (เหมือน
        //   `refx_render::device::headless_device` ซึ่งเป็น `#[cfg(test)]`
        //   ของ crate นั้น จึงเรียกจากที่นี่ไม่ได้)
        let ask = |fallback| {
            pollster_lite(instance.request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::LowPower,
                compatible_surface: None,
                force_fallback_adapter: fallback,
            }))
            .and_then(Result::ok)
        };
        let adapter = ask(false).or_else(|| ask(true))?;
        let info = adapter.get_info();
        let limits = adapter.limits();
        let (device, queue) = pollster_lite(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("refx-export-test"),
            required_features: wgpu::Features::empty(),
            required_limits: limits,
            ..Default::default()
        }))?
        .ok()?;

        let caps = refx_render::device::GpuCapabilities {
            adapter_name: info.name.clone(),
            backend: info.backend,
            device_type: info.device_type,
            bc_compression: false,
            max_texture_dimension_2d: device.limits().max_texture_dimension_2d,
        };
        let allocator = TextureAllocator::new(&caps);
        let mut atlas = ThumbnailAtlas::new(&device, &allocator, 4).ok()?;
        atlas.resize(&device, 1).ok()?;
        Some((device, queue, allocator, atlas, instance))
    }

    /// รอ future ที่ไม่มี waker จริง — พอสำหรับ `request_adapter`/`request_device`
    /// ซึ่ง wgpu ทำเสร็จทันทีบน backend ที่เราใช้ (เหมือน `block_on` ของ `device.rs`)
    fn pollster_lite<F: std::future::Future>(future: F) -> Option<F::Output> {
        use std::task::{Context, Poll, Waker};
        let mut future = Box::pin(future);
        let mut context = Context::from_waker(Waker::noop());
        for _ in 0..10_000 {
            if let Poll::Ready(value) = future.as_mut().poll(&mut context) {
                return Some(value);
            }
            std::thread::yield_now();
        }
        None
    }

    fn quads(region: Rect, count: u32) -> Vec<QuadInstance> {
        let size = region.size();
        (0..count)
            .map(|i| {
                let t = i as f32 / count as f32;
                QuadInstance {
                    transform: [
                        size.x * 0.4,
                        0.0,
                        0.0,
                        size.y * 0.4,
                        region.min.x + size.x * t * 0.5,
                        region.min.y + size.y * t * 0.5,
                    ],
                    uv_rect: [0.0, 0.0, 1.0, 1.0],
                    tint: pack_tint([t, 1.0 - t, 0.5, 1.0]),
                    layer: 0,
                    flags: refx_render::instance::flags::PLACEHOLDER,
                    adjust: QuadInstance::NEUTRAL_ADJUST,
                    reserved: 0,
                }
            })
            .collect()
    }

    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "refx-uiexport-{tag}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn plain_rename(from: &std::path::Path, to: &std::path::Path) -> std::io::Result<()> {
        std::fs::rename(from, to)
    }

    /// ★★★ **board ว่าง = ปฏิเสธ ไม่ใช่ไฟล์ 0 ไบต์** (`docs/07 §6` ข้อ 5)
    ///
    /// ข้อนี้ไม่ต้องใช้ GPU เลย เพราะการปฏิเสธต้องเกิด**ก่อน**จองอะไรทั้งสิ้น
    #[test]
    fn an_empty_board_is_refused_before_anything_is_allocated() {
        let Some((device, queue, allocator, atlas, _instance)) = scene() else {
            eprintln!("ข้าม: ไม่มี GPU adapter");
            return;
        };
        let dir = temp_dir("empty");
        let path = dir.join("empty.png");

        // ★ ก้อนที่มีอยู่แต่ว่างเปล่าก็ยังนับเป็นว่าง — เป็นสภาพที่เกิดจริงเมื่อ
        //   ทุกใบบน board เป็นโน้ตข้อความ (egui วาด ไม่ใช่ quad)
        let empty = [DrawBatch {
            bind_group: atlas.bind_group(),
            instances: &[],
        }];
        for batches in [&empty[..], &[][..]] {
            let err = export_board(
                GpuAccess {
                    device: &device,
                    queue: &queue,
                    allocator: &allocator,
                    atlas_layout: atlas.bind_group_layout(),
                },
                batches,
                &ExportRequest {
                    path: path.clone(),
                    format: ExportFormat::Png { transparent: true },
                    size: (256, 256),
                    region: Rect::from_corners(Vec2::ZERO, Vec2::new(256.0, 256.0)),
                    background: [0, 0, 0, 0],
                },
                JobSignals::just_cancel(&AtomicBool::new(false)),
                plain_rename,
            )
            .unwrap_err();
            assert!(matches!(err, ExportJobError::NothingToExport), "ได้ {err:?}");
        }
        assert!(!path.exists(), "เขียนไฟล์ทั้งที่ไม่มีอะไรให้ export");
    }

    /// ★★★ **`NUL.png` ต้องถูกปฏิเสธ ไม่ใช่ "สำเร็จ" แบบเงียบ**
    ///
    /// Windows เขียนลง `NUL.png` **สำเร็จแล้วทิ้งข้อมูล** → ผู้ใช้เชื่อว่า export
    /// แล้วแต่ไม่มีอะไรเลย = งานหายแบบที่ I-3 ห้าม · ด่านต้องอยู่ใน
    /// [`export_board`] เอง ไม่ใช่ในตัวเรียก
    #[test]
    fn a_save_target_that_would_silently_swallow_the_file_is_refused() {
        let Some((device, queue, allocator, atlas, _instance)) = scene() else {
            eprintln!("ข้าม: ไม่มี GPU adapter");
            return;
        };
        let dir = temp_dir("nul");
        let instances = quads(Rect::from_corners(Vec2::ZERO, Vec2::new(256.0, 256.0)), 4);
        let batches = [DrawBatch {
            bind_group: atlas.bind_group(),
            instances: &instances,
        }];

        for evil in ["NUL.png", "nul", "COM1.png"] {
            let path = dir.join(evil);
            let err = export_board(
                GpuAccess {
                    device: &device,
                    queue: &queue,
                    allocator: &allocator,
                    atlas_layout: atlas.bind_group_layout(),
                },
                &batches,
                &ExportRequest {
                    path: path.clone(),
                    format: ExportFormat::Png { transparent: true },
                    size: (256, 256),
                    region: Rect::from_corners(Vec2::ZERO, Vec2::new(256.0, 256.0)),
                    background: [0, 0, 0, 0],
                },
                JobSignals::just_cancel(&AtomicBool::new(false)),
                plain_rename,
            )
            .unwrap_err();
            assert!(
                matches!(
                    err,
                    ExportJobError::BadTarget(refx_io::validate::SecError::Device)
                ),
                "{evil}: ได้ {err:?}"
            );
        }

        // ★ negative control ของด่านเดียวกัน: ที่ปกติต้องผ่านได้จริง ไม่งั้น
        //   ข้อบนพิสูจน์แค่ว่า "ทุกอย่างถูกปฏิเสธ" ซึ่งไม่ใช่สิ่งที่เราต้องการ
        let fine = dir.join("moodboard.png");
        export_board(
            GpuAccess {
                device: &device,
                queue: &queue,
                allocator: &allocator,
                atlas_layout: atlas.bind_group_layout(),
            },
            &batches,
            &ExportRequest {
                path: fine.clone(),
                format: ExportFormat::Png { transparent: true },
                size: (256, 256),
                region: Rect::from_corners(Vec2::ZERO, Vec2::new(256.0, 256.0)),
                background: [0, 0, 0, 0],
            },
            JobSignals::just_cancel(&AtomicBool::new(false)),
            plain_rename,
        )
        .expect("ชื่อไฟล์ปกติต้อง export ได้");
        assert!(fine.exists());
    }

    /// ★★★ **ปุ่มยกเลิกต้องตอบสนองภายในไม่กี่ร้อย ms ที่ขนาดใหญ่ที่สุด**
    ///
    /// `docs/07 §6`: การยกเลิกเกิดที่ **ปลายทาง** ไม่ใช่ที่ลูปของเรา · ข้อนี้คือ
    /// การวัดว่ากฎนั้นให้ผลจริงเท่าไหร่ · ★ **พิมพ์เวลา ไม่ assert เวลา**
    /// (`docs/08 §3.9` ข้อ 5b) — สิ่งที่ assert ได้คือ *ผลลัพธ์*: ยกเลิกแล้ว
    /// ต้องไม่มีไฟล์ปลายทาง และไม่มีไฟล์ครึ่งใบ
    ///
    /// ★★ ระหว่างรอ เธรดนี้ (ที่แทน UI thread) **ไม่ถูกบล็อกเลย** — มันวนถาม
    /// `finished()` ซึ่งเป็น `try_recv` · ถ้าใครเปลี่ยนไปใช้ `recv()` ข้อนี้จะยัง
    /// เขียวแต่แอปจริงจะค้าง จึงนับจำนวนรอบที่วนได้ไว้เป็นหลักฐานด้วย
    #[test]
    fn cancelling_a_big_export_stops_within_a_moment_and_leaves_no_file() {
        let Some((device, queue, allocator, atlas, _instance)) = scene() else {
            eprintln!("ข้าม: ไม่มี GPU adapter");
            return;
        };
        let dir = temp_dir("cancel-job");
        let path = dir.join("huge.jpg");
        let region = Rect::from_corners(Vec2::ZERO, Vec2::new(16384.0, 16384.0));
        let job = ExportJob::spawn(
            OwnedGpu {
                device: device.clone(),
                queue: queue.clone(),
                allocator: allocator.clone(),
                atlas_layout: atlas.bind_group_layout().clone(),
            },
            vec![OwnedBatch {
                bind_group: atlas.bind_group().clone(),
                instances: quads(region, 64),
            }],
            ExportRequest {
                path: path.clone(),
                format: ExportFormat::Jpeg { quality: 90 },
                size: (16384, 16384),
                region,
                background: [24, 24, 28, 255],
            },
            plain_rename,
            // ★ ไม่มีตัวปลุกในเทสต์ — ไม่มี event loop ให้ปลุก
            None,
        )
        .expect("ยิงงานไม่สำเร็จ");

        // ★ ปล่อยให้มันทำงานไปก่อน แล้วค่อยกดยกเลิก — ยกเลิกตั้งแต่ยังไม่เริ่ม
        //   เป็นคนละเส้นทาง (มีเทสต์ของมันเองใน `refx-asset`)
        let mut spins = 0u32;
        while job.progress().0 == 0 && job.elapsed() < std::time::Duration::from_secs(20) {
            spins += 1;
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        let (done, total) = job.progress();
        assert!(done > 0, "งานยังไม่ทันเริ่มก็หมดเวลา");

        let pressed = std::time::Instant::now();
        job.cancel();
        assert!(job.cancelling());

        let outcome = loop {
            if let Some(result) = job.finished() {
                break result;
            }
            spins += 1;
            assert!(
                pressed.elapsed() < std::time::Duration::from_secs(30),
                "กดยกเลิกแล้วไม่หยุดสักที"
            );
            std::thread::sleep(std::time::Duration::from_millis(1));
        };
        let stopped_in = pressed.elapsed();

        println!(
            "16384² JPEG: ทำไป {done}/{total} แถบ แล้วกดยกเลิก → หยุดใน {stopped_in:?} \
             (เธรดที่รออยู่วนได้ {spins} รอบ = ไม่เคยถูกบล็อก)"
        );
        assert!(
            matches!(
                outcome,
                Err(ExportJobError::Write(
                    refx_asset::export::ExportError::Cancelled
                ))
            ),
            "ได้ {outcome:?}"
        );
        assert!(!path.exists(), "ยกเลิกแล้วยังมีไฟล์ปลายทาง");
        assert!(!dir.join("huge.jpg.tmp").exists(), "ยกเลิกแล้วเหลือไฟล์ครึ่งใบ");
        assert!(spins > 10, "วนได้แค่ {spins} รอบ — เธรดถูกบล็อกอยู่หรือเปล่า");
    }

    /// งานที่ปล่อยให้จบต้องได้ไฟล์จริง และขนาดต้องตรงกับที่รายงาน
    #[test]
    fn a_job_that_runs_to_completion_reports_the_file_it_wrote() {
        let Some((device, queue, allocator, atlas, _instance)) = scene() else {
            eprintln!("ข้าม: ไม่มี GPU adapter");
            return;
        };
        let dir = temp_dir("job-done");
        let path = dir.join("board.png");
        let region = Rect::from_corners(Vec2::ZERO, Vec2::new(1024.0, 768.0));
        let job = ExportJob::spawn(
            OwnedGpu {
                device: device.clone(),
                queue: queue.clone(),
                allocator: allocator.clone(),
                atlas_layout: atlas.bind_group_layout().clone(),
            },
            vec![OwnedBatch {
                bind_group: atlas.bind_group().clone(),
                instances: quads(region, 8),
            }],
            ExportRequest {
                path: path.clone(),
                format: ExportFormat::Png { transparent: false },
                size: (1024, 768),
                region,
                background: [255, 255, 255, 255],
            },
            plain_rename,
            // ★ ไม่มีตัวปลุกในเทสต์ — ไม่มี event loop ให้ปลุก
            None,
        )
        .unwrap();
        assert_eq!(job.name(), "board.png");

        let stats = loop {
            if let Some(result) = job.finished() {
                break result.expect("export ล้ม");
            }
            assert!(
                job.elapsed() < std::time::Duration::from_secs(30),
                "ไม่จบสักที"
            );
            std::thread::sleep(std::time::Duration::from_millis(1));
        };
        assert!(stats.bytes > 0);
        assert_eq!(std::fs::metadata(&path).unwrap().len(), stats.bytes);
        assert_eq!(job.progress(), (stats.bands, stats.bands), "แถบไม่ครบ");
        // ★ ถามซ้ำหลังรับผลไปแล้วต้องไม่ค้าง — UI ถามทุกเฟรม
        assert!(matches!(job.finished(), Some(Err(ExportJobError::Lost))));
    }
}
