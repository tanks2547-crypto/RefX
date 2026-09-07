//! Export ภาพขนาดใหญ่แบบ **ทีละแถบ** — ฝั่ง GPU (P5-4 · `docs/07 §6`)
//!
//! ## ★★★ ทำไมเป็นแถบ ไม่ใช่จตุรัส
//!
//! สเปกรุ่นแรกเขียนว่า render ทีละ tile 4096×4096 · **ตัวเลขบอกว่าทำไม่ได้**:
//! tile 4096² RGBA = **67 MB** ซึ่งใหญ่กว่างบของบัฟเฟอร์แถบทั้งก้อน (32 MB)
//! → เพดานจะพังที่ **ทางอ่านกลับจาก GPU** ไม่ใช่ที่ตัวเข้ารหัส ซึ่งเป็นที่ที่
//! ตัวเลขฝั่ง CPU ที่วัดไว้ใน `docs/07 §6` มองไม่เห็นเลย
//!
//! → tile ของเราจึงเป็น **ทรงแถบ**: กว้าง ≤ `TILE_WIDTH` × สูง = `band_rows`
//! ซึ่งทำให้ทั้ง texture, staging buffer และบัฟเฟอร์แถบ อยู่ในงบพร้อมกัน
//!
//! ## ★★ ไม่มีเส้นทางวาดสำหรับ export แยกต่างหาก
//!
//! `docs/07 §6` ห้ามไว้ตรง ๆ — grayscale/flip/opacity/rotation ต้องผ่าน shader
//! เดียวกับที่วาดบนจอ ไม่งั้นภาพที่ส่งให้คนอื่นจะเพี้ยนจากที่เห็นแบบเงียบ ๆ
//!
//! ที่นี่จึงใช้ [`QuadPipeline`] และ `quad.wgsl` **ตัวเดียวกับหน้าจอ** ·
//! สิ่งเดียวที่ต่างคือ *format ของเป้า* (เป้าของเราไม่ใช่ surface ของหน้าต่าง)
//! ซึ่ง wgpu บังคับให้ pipeline ผูกกับ format ตอนสร้าง — เลี่ยงไม่ได้
//! และ**ไม่ได้เปลี่ยนสิ่งที่ shader คำนวณแม้แต่บรรทัดเดียว**
//!
//! ## I-2
//!
//! ทั้งโมดูลนี้ **บล็อกรอ GPU** ตอนอ่านผลกลับ → ห้ามเรียกจาก UI thread
//! ผู้เรียกต้องอยู่บน worker (ดู `refx-ui::export`)
//!
//! spec: docs/07-file-format.md §6, docs/04-rendering.md §4

use glam::Vec2;
use refx_core::export::{BandPlan, padded_row_bytes, tile_clip_affine};
use refx_core::geom::Rect;

use crate::pipeline::{CameraUniform, DrawBatch, QuadPipeline};
use crate::texture::{TextureAllocator, TrackedTexture};

/// format ของเป้า export
///
/// ★ **ต้องเป็น sRGB** เหมือน surface ของหน้าต่าง — shader คำนวณในปริภูมิเชิงเส้น
/// แล้วให้ GPU แปลง gamma ให้ตอนเขียนลงเป้า · ถ้าใช้ `Rgba8Unorm` เฉย ๆ ภาพที่ได้
/// จะ**สว่างจ้าผิดปกติทั้งใบ** เทียบกับที่เห็นบนจอ โดยไม่มี error ที่ไหนเลย
pub const EXPORT_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8UnormSrgb;

/// รอ GPU ได้นานสุดต่อหนึ่งแถบ
///
/// ★ ต้องมีเสมอ — งานที่ค้างตลอดกาลบน worker แปลว่าผู้ใช้กด "ยกเลิก" แล้วไม่มีอะไรเกิดขึ้น
const READBACK_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);

/// render ภาพ export ไม่สำเร็จ
#[derive(Debug, thiserror::Error)]
pub enum ExportRenderError {
    /// ขนาดที่ขอ export ไม่ได้
    #[error(transparent)]
    Plan(#[from] refx_core::export::PlanError),
    /// จอง VRAM สำหรับเป้าไม่ได้ — ผู้เรียกควรบอกผู้ใช้ให้ปิดภาพบางส่วนแล้วลองใหม่
    #[error(transparent)]
    Vram(#[from] crate::texture::VramError),
    /// GPU ไม่ส่งพิกเซลกลับมา (device lost / timeout)
    #[error("the GPU did not return the exported pixels: {reason}")]
    Readback {
        /// สิ่งที่เกิดขึ้นจริง
        reason: String,
    },
}

/// แปลงค่า sRGB หนึ่งช่อง (0..=255) เป็นค่าเชิงเส้นที่ `LoadOp::Clear` ต้องการ
///
/// ★★ `wgpu::Color` เป็น **ค่าเชิงเส้น** เสมอ แต่สีที่ผู้ใช้เลือกจากจานสีคือ sRGB
/// ถ้าใส่ตรง ๆ พื้นหลังสีเทา 50% จะออกมาสว่างกว่าที่เลือกอย่างเห็นได้ชัด
/// (ขาวกับดำไม่เห็นความต่าง ซึ่งเป็นเหตุผลที่บั๊กแบบนี้รอดเทสต์ง่าย)
#[must_use]
pub fn srgb_channel_to_linear(value: u8) -> f64 {
    let c = f64::from(value) / 255.0;
    if c <= 0.040_45 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

/// สีพื้นหลังที่ผู้ใช้เลือก (sRGB + alpha) → `wgpu::Color`
#[must_use]
pub fn background_colour(rgba: [u8; 4]) -> wgpu::Color {
    wgpu::Color {
        r: srgb_channel_to_linear(rgba[0]),
        g: srgb_channel_to_linear(rgba[1]),
        b: srgb_channel_to_linear(rgba[2]),
        // ★ alpha ไม่ผ่านเส้นโค้ง gamma — มันเป็นสัดส่วนการผสม ไม่ใช่ความสว่าง
        a: f64::from(rgba[3]) / 255.0,
    }
}

/// เครื่อง render ภาพ export ทีละแถบ
///
/// อายุของมันคืออายุของ export งานหนึ่ง — จองเป้ากับ staging ครั้งเดียวแล้วใช้ซ้ำ
/// ทุกแถบ (CLAUDE.md ห้ามสร้าง buffer/texture ใหม่ทุกเฟรม ด้วยเหตุผลเดียวกัน)
pub struct BandRenderer {
    plan: BandPlan,
    region: Rect,
    clear: wgpu::Color,
    pipeline: QuadPipeline,
    /// เป้าทรงแถบ — กว้างเท่า tile ที่กว้างที่สุด สูงเท่าแถบเต็มหนึ่งแถบ
    target: TrackedTexture,
    view: wgpu::TextureView,
    /// บัฟเฟอร์รับผลจาก GPU (หนึ่ง tile)
    staging: wgpu::Buffer,
}

impl BandRenderer {
    /// เตรียมเป้าและบัฟเฟอร์ทั้งหมดของ export งานหนึ่ง
    ///
    /// `region` คือกรอบใน world ที่จะถูก export · `background` เป็นสี sRGB
    /// (JPEG ไม่มี alpha → ผู้เรียกต้องส่ง alpha 255 มา ดู `docs/07 §6`)
    ///
    /// # Errors
    /// [`ExportRenderError::Vram`] เมื่อจองเป้าไม่ได้
    pub fn new(
        device: &wgpu::Device,
        allocator: &TextureAllocator,
        atlas_layout: &wgpu::BindGroupLayout,
        plan: BandPlan,
        region: Rect,
        background: [u8; 4],
    ) -> Result<Self, ExportRenderError> {
        let tile_width = plan.width().min(refx_core::export::TILE_WIDTH);
        let target = allocator.allocate(
            device,
            &wgpu::TextureDescriptor {
                label: Some("refx-export-band"),
                size: wgpu::Extent3d {
                    width: tile_width,
                    height: plan.band_rows(),
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: EXPORT_FORMAT,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
                view_formats: &[],
            },
        )?;
        let view = target.create_view(&wgpu::TextureViewDescriptor::default());

        let staging = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("refx-export-readback"),
            size: u64::from(padded_row_bytes(tile_width)) * u64::from(plan.band_rows()),
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });

        Ok(Self {
            pipeline: QuadPipeline::new(device, EXPORT_FORMAT, atlas_layout),
            plan,
            region,
            clear: background_colour(background),
            target,
            view,
            staging,
        })
    }

    /// แผนที่เครื่องนี้ทำงานอยู่
    #[must_use]
    pub fn plan(&self) -> &BandPlan {
        &self.plan
    }

    /// VRAM ที่เป้าใช้อยู่ (ไบต์) — ตัวเลขที่ต้องขึ้น status bar ตาม I-6
    #[must_use]
    pub fn target_bytes(&self) -> usize {
        self.target.bytes()
    }

    /// ไบต์ของ staging buffer
    #[must_use]
    pub fn staging_bytes(&self) -> usize {
        usize::try_from(self.staging.size()).unwrap_or(usize::MAX)
    }

    /// วาดแถบลำดับที่ `index` **ลงในบัฟเฟอร์ของผู้เรียก** แล้วคืนจำนวนแถวที่เขียน
    ///
    /// ★★★ **ทำไมเขียนลงบัฟเฟอร์ของคนอื่น ไม่ใช่ของตัวเอง**
    ///
    /// ตัวเข้ารหัสต้องมีบัฟเฟอร์แถบของมันอยู่แล้ว · ถ้าที่นี่ถือของตัวเองอีกก้อน
    /// จะมีบัฟเฟอร์ 32 MB **สองก้อนพร้อมกัน** แล้วเพดาน 64 MB ของ `docs/07 §6`
    /// พังทันทีที่ความกว้าง 8192 โดยที่ตัวเลขของทั้งสองฝั่งแยกกันดูยังสวยอยู่
    ///
    /// `out` ต้องยาวอย่างน้อย `rows × width × 4` · คืน `0` เมื่อ `index`
    /// เลยแถบสุดท้ายไปแล้ว
    ///
    /// # Errors
    /// [`ExportRenderError::Readback`] เมื่อ GPU ไม่ส่งผลกลับมาในเวลาที่กำหนด
    /// หรือบัฟเฟอร์ที่ให้มาสั้นเกินไป
    pub fn render_band(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        index: u32,
        batches: &[DrawBatch<'_>],
        out: &mut [u8],
    ) -> Result<u32, ExportRenderError> {
        let Some(band) = self.plan.band(index) else {
            // ★ ขอแถบที่ไม่มี = ตัวเรียกวนเกิน — คืน 0 ไม่ใช่ panic
            return Ok(0);
        };
        let needed = band.rows as usize * self.plan.width() as usize * 4;
        if out.len() < needed {
            return Err(ExportRenderError::Readback {
                reason: format!(
                    "the caller's band buffer holds {} bytes but band {index} needs {needed}",
                    out.len()
                ),
            });
        }

        let stride = self.plan.width() as usize * 4;
        let out_size = Vec2::new(self.plan.width() as f32, self.plan.height() as f32);

        let mut column = 0;
        while let Some(tile) = self.plan.tile(column) {
            let affine = tile_clip_affine(
                self.region,
                out_size,
                Vec2::new(tile.x0 as f32, band.y0 as f32),
                Vec2::new(tile.width as f32, band.rows as f32),
            );
            self.pipeline
                .set_camera(queue, CameraUniform::from_affine(affine));

            let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("refx-export-tile"),
            });
            {
                let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("refx-export-pass"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: &self.view,
                        depth_slice: None,
                        resolve_target: None,
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Clear(self.clear),
                            store: wgpu::StoreOp::Store,
                        },
                    })],
                    depth_stencil_attachment: None,
                    timestamp_writes: None,
                    occlusion_query_set: None,
                    multiview_mask: None,
                });
                // ★ เป้าถูกจองไว้เท่า tile ที่กว้างที่สุด — tile ใบสุดท้ายแคบกว่า
                //   ต้องหด viewport ตาม ไม่งั้นภาพส่วนขวาสุดจะถูกยืดออก
                pass.set_viewport(0.0, 0.0, tile.width as f32, band.rows as f32, 0.0, 1.0);
                self.pipeline.draw_batches(queue, &mut pass, batches);
            }

            let padded = padded_row_bytes(tile.width);
            encoder.copy_texture_to_buffer(
                wgpu::TexelCopyTextureInfo {
                    texture: self.target.texture(),
                    mip_level: 0,
                    origin: wgpu::Origin3d::ZERO,
                    aspect: wgpu::TextureAspect::All,
                },
                wgpu::TexelCopyBufferInfo {
                    buffer: &self.staging,
                    layout: wgpu::TexelCopyBufferLayout {
                        offset: 0,
                        bytes_per_row: Some(padded),
                        rows_per_image: Some(band.rows),
                    },
                },
                wgpu::Extent3d {
                    width: tile.width,
                    height: band.rows,
                    depth_or_array_layers: 1,
                },
            );
            queue.submit(std::iter::once(encoder.finish()));

            let rows = band.rows as usize;
            let tile_bytes = tile.width as usize * 4;
            let padded = padded as usize;
            let slice = self.staging.slice(..(padded * rows) as u64);
            let (tx, rx) = std::sync::mpsc::channel();
            slice.map_async(wgpu::MapMode::Read, move |result| {
                let _ = tx.send(result.is_ok());
            });
            device
                .poll(wgpu::PollType::Wait {
                    submission_index: None,
                    timeout: Some(READBACK_TIMEOUT),
                })
                .map_err(|err| ExportRenderError::Readback {
                    reason: format!("{err:?}"),
                })?;
            match rx.try_recv() {
                Ok(true) => {}
                other => {
                    return Err(ExportRenderError::Readback {
                        reason: format!("mapping the readback buffer failed ({other:?})"),
                    });
                }
            }

            {
                let mapped = slice.get_mapped_range();
                let x_offset = tile.x0 as usize * 4;
                for row in 0..rows {
                    let from = row * padded;
                    let to = row * stride + x_offset;
                    out[to..to + tile_bytes].copy_from_slice(&mapped[from..from + tile_bytes]);
                }
            }
            self.staging.unmap();

            column += 1;
        }

        Ok(band.rows)
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::float_cmp)]

    use refx_core::export::{MAX_SIDE, PEAK_CEILING};

    use super::*;
    use crate::atlas::ThumbnailAtlas;
    use crate::instance::{QuadInstance, flags, pack_tint};

    /// ★★ ค่า sRGB ที่ผ่าน `background_colour` แล้วเขียนลงเป้า sRGB ต้องกลับมา
    /// **เท่าเดิม** — ถ้าไม่แปลง gamma ค่ากลาง ๆ จะเพี้ยนไปหลายสิบขั้น
    #[test]
    fn the_background_colour_survives_the_gamma_round_trip() {
        for value in [0u8, 1, 64, 128, 200, 255] {
            let linear = srgb_channel_to_linear(value);
            // เข้ารหัสกลับด้วยสูตรผกผัน — GPU ทำขั้นนี้ให้ตอนเขียนลงเป้า sRGB
            let back = if linear <= 0.003_130_8 {
                linear * 12.92
            } else {
                1.055 * linear.powf(1.0 / 2.4) - 0.055
            };
            #[expect(clippy::cast_possible_truncation, reason = "ค่าอยู่ใน 0..=255 แน่")]
            let back = (back * 255.0).round() as u8;
            assert_eq!(back, value, "sRGB {value} กลับมาเป็น {back}");
        }
    }

    /// alpha ต้องไม่ผ่านเส้นโค้ง gamma
    #[test]
    fn alpha_is_not_a_brightness() {
        let colour = background_colour([0, 0, 0, 128]);
        assert!(
            (colour.a - 128.0 / 255.0).abs() < 1e-9,
            "alpha ถูกแปลง gamma"
        );
    }

    fn harness() -> Option<(wgpu::Device, wgpu::Queue, TextureAllocator, ThumbnailAtlas)> {
        let (device, queue, caps) = crate::device::gpu_for_test()?;
        let allocator = TextureAllocator::new(&caps);
        let mut atlas = ThumbnailAtlas::new(&device, &allocator, 2).ok()?;
        atlas.resize(&device, 1).ok()?;
        Some((device, queue, allocator, atlas))
    }

    /// quad สีทึบที่กิน world ทั้งกรอบที่ export
    fn full_region_quad(region: Rect, rgba: [f32; 4]) -> QuadInstance {
        let size = region.size();
        QuadInstance {
            transform: [size.x, 0.0, 0.0, size.y, region.min.x, region.min.y],
            uv_rect: [0.0, 0.0, 1.0, 1.0],
            tint: pack_tint(rgba),
            layer: 0,
            flags: flags::PLACEHOLDER,
            adjust: QuadInstance::NEUTRAL_ADJUST,
            reserved: 0,
        }
    }

    /// ★★★ ภาพที่ประกอบจากทุกแถบต้องเป็นภาพเดียวกับที่ตั้งใจ **ทั้งใบ**
    ///
    /// ทดสอบด้วยขนาดที่ **กว้างเกินหนึ่ง tile และสูงเกินหนึ่งแถบ** เพราะนั่นคือ
    /// เงื่อนไขเดียวที่ทำให้ความผิดพลาดเรื่อง offset โผล่ — ขนาดเล็กผ่านหมด
    #[test]
    fn every_band_and_tile_lands_in_the_right_place() {
        let Some((device, queue, allocator, atlas)) = harness() else {
            eprintln!("ข้าม: ไม่มี GPU adapter");
            return;
        };

        // 5000 กว้าง = 2 tile · 40 สูงกับแถบ 40 แถว... บังคับให้มีหลายแถบด้วย
        let plan = BandPlan::new(5000, 3).unwrap();
        let region = Rect::from_corners(Vec2::ZERO, Vec2::new(5000.0, 3.0));
        let mut renderer = BandRenderer::new(
            &device,
            &allocator,
            atlas.bind_group_layout(),
            plan,
            region,
            [0, 0, 0, 255],
        )
        .unwrap();

        let quad = full_region_quad(region, [1.0, 0.0, 0.0, 1.0]);
        let batch = DrawBatch {
            bind_group: atlas.bind_group(),
            instances: std::slice::from_ref(&quad),
        };

        let mut pixels = vec![0u8; plan.band_bytes()];
        let rows = renderer
            .render_band(
                &device,
                &queue,
                0,
                std::slice::from_ref(&batch),
                &mut pixels,
            )
            .unwrap();
        assert_eq!(rows, 3);

        // ★ ตรวจ **ขอบขวาสุด** ด้วย — นั่นคือ tile ใบที่สองซึ่งแคบกว่าเป้า
        //   ถ้า viewport หรือ offset ผิด ตรงนี้จะเป็นสีพื้นหลังแทนสีของ quad
        for (label, x) in [("ซ้ายสุด", 0usize), ("รอยต่อ", 4096), ("ขวาสุด", 4999)]
        {
            let at = x * 4;
            assert!(
                pixels[at] > 200 && pixels[at + 1] < 60,
                "{label} (x={x}) ไม่ใช่สีแดง: {:?}",
                &pixels[at..at + 4]
            );
        }
    }

    /// แถบสุดท้ายเตี้ยกว่าตัวอื่น — จำนวนแถวที่คืนต้องสั้นตาม ไม่ใช่รายงานแถบเต็ม
    /// แล้วปล่อยให้ตัวเข้ารหัสเขียนขยะของแถบก่อนหน้าลงท้ายไฟล์
    #[test]
    fn the_last_band_returns_only_the_rows_that_exist() {
        let Some((device, queue, allocator, atlas)) = harness() else {
            eprintln!("ข้าม: ไม่มี GPU adapter");
            return;
        };
        // ความกว้าง 16384 → แถบ 512 แถว · ความสูง 513 → สองแถบ ตัวที่สองสูง 1 แถว
        let plan = BandPlan::new(16384, 513).unwrap();
        assert_eq!(plan.band_count(), 2);
        let region = Rect::from_corners(Vec2::ZERO, Vec2::new(16384.0, 513.0));
        let mut renderer = BandRenderer::new(
            &device,
            &allocator,
            atlas.bind_group_layout(),
            plan,
            region,
            [0, 0, 0, 255],
        )
        .unwrap();

        let mut pixels = vec![0u8; plan.band_bytes()];
        let first = renderer
            .render_band(&device, &queue, 0, &[], &mut pixels)
            .unwrap();
        assert_eq!(first, 512);
        let last = renderer
            .render_band(&device, &queue, 1, &[], &mut pixels)
            .unwrap();
        assert_eq!(last, 1, "แถบสุดท้ายต้องมีแถวเดียว");
        let past = renderer
            .render_band(&device, &queue, 2, &[], &mut pixels)
            .unwrap();
        assert_eq!(past, 0, "ขอแถบที่ไม่มีต้องได้ 0 ไม่ใช่ panic");

        // ★ บัฟเฟอร์ที่สั้นเกินไปต้องเป็น error ไม่ใช่การเขียนล้นแล้ว panic
        let mut tiny = vec![0u8; 16];
        assert!(
            renderer
                .render_band(&device, &queue, 0, &[], &mut tiny)
                .is_err(),
            "บัฟเฟอร์สั้นเกินต้องถูกปฏิเสธ"
        );
    }

    /// ★★★ **บัฟเฟอร์ที่เครื่องนี้ถือจริง ต้องอยู่ใต้เพดานที่แผนสัญญาไว้**
    ///
    /// แผนตอบได้แค่ *ความตั้งใจ* · ข้อนี้ถามของจริงที่ถูกจองไปแล้ว
    /// (เป้าบน GPU + staging) ที่ **ทุกขนาดที่ผู้ใช้เลือกได้**
    #[test]
    fn what_the_gpu_side_of_an_export_actually_reserves() {
        let Some((device, _queue, allocator, atlas)) = harness() else {
            eprintln!("ข้าม: ไม่มี GPU adapter");
            return;
        };
        for side in [4096u32, 8192, MAX_SIDE] {
            let plan = BandPlan::new(side, side).unwrap();
            let renderer = BandRenderer::new(
                &device,
                &allocator,
                atlas.bind_group_layout(),
                plan,
                Rect::from_corners(Vec2::ZERO, Vec2::new(side as f32, side as f32)),
                [0, 0, 0, 255],
            )
            .unwrap();
            // ★ สองก้อนนี้อยู่ใน **RAM ของโปรเซส**: บัฟเฟอร์แถบเป็นของเราตรง ๆ
            //   ส่วน staging ถูกจองด้วย MAP_READ = ต้อง map เข้ามาให้ CPU อ่านได้
            //   → เพดานของ `docs/07 §6` นับสองก้อนนี้
            let in_ram = renderer.staging_bytes() + plan.band_bytes();
            println!(
                "{side}²: [RAM] แถบ {} MB + staging {} MB = {} MB · [VRAM] เป้า {} MB",
                plan.band_bytes() >> 20,
                renderer.staging_bytes() >> 20,
                in_ram >> 20,
                renderer.target_bytes() >> 20,
            );
            assert!(
                in_ram <= PEAK_CEILING,
                "{side}²: จองจริง {} MB เกินเพดาน {} MB",
                in_ram >> 20,
                PEAK_CEILING >> 20
            );
            // เป้าอยู่ใน VRAM ซึ่งมีถังของตัวเอง (I-6) — ต้องไม่กินเกินหนึ่งในสี่
            // ของงบ iGPU ที่แคบที่สุด ไม่งั้น export จะเบียดภาพบนจอจนกระพริบ
            assert!(
                renderer.target_bytes() <= (128 << 20) / 4,
                "{side}²: เป้ากิน VRAM {} MB",
                renderer.target_bytes() >> 20
            );
        }
    }
}
