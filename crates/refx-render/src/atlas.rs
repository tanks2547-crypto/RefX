//! Thumbnail atlas — `Texture2DArray` ที่เก็บภาพย่อทุกภาพบน board
//!
//! ทั้ง board วาดด้วย **1 bind group 1 draw call** — นี่คือเหตุผลที่ 1000+ ภาพยังลื่น
//!
//! จัดสรรด้วย free-list ธรรมดา: ทุกช่องขนาดเท่ากัน (128×128) จึงไม่มี fragmentation
//! และไม่ต้องใช้ bin-packing
//!
//! **ใช้ `Rgba8UnormSrgb` อย่างเดียว ไม่มีสาขา BC7** (ตัดสิน 27 ก.ค. 2026, docs/04 §4)
//! เหตุผลหลักไม่ใช่เรื่องหา crate ไม่ได้ แต่เพราะ BC7 encode ใช้เวลาระดับร้อย ms
//! ต่อ tile 2048² ซึ่งชนกับสัญญาข้อแรกของโปรแกรม — ภาพต้องขึ้นทันทีที่ลากเข้ามา
//!
//! ★ ทุก texture ต้องผ่าน [`TextureAllocator`] เท่านั้น (I-6 / CLAUDE.md)
//!
//! spec: docs/04-rendering.md §4

use crate::device::GpuCapabilities;
use crate::texture::{TextureAllocator, TrackedTexture, VramError};

/// ขนาดช่องหนึ่งช่อง (docs/04 §4)
pub const SLOT_SIZE: u32 = 128;
/// ขนาดของ texture หนึ่ง layer
pub const LAYER_SIZE: u32 = 2048;
/// จำนวนช่องต่อ layer = 16 × 16
pub const SLOTS_PER_LAYER: u32 = (LAYER_SIZE / SLOT_SIZE) * (LAYER_SIZE / SLOT_SIZE);

/// ตำแหน่งของภาพหนึ่งภาพใน atlas
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AtlasSlot {
    /// ชั้นใน texture array
    pub layer: u32,
    /// ดัชนีช่องภายใน layer (0..`SLOTS_PER_LAYER`)
    pub index: u32,
}

impl AtlasSlot {
    /// พิกัด uv ของช่องนี้ `[u0, v0, u1, v1]`
    #[must_use]
    pub fn uv_rect(self) -> [f32; 4] {
        let per_row = LAYER_SIZE / SLOT_SIZE;
        let col = self.index % per_row;
        let row = self.index / per_row;
        let step = SLOT_SIZE as f32 / LAYER_SIZE as f32;
        let u0 = col as f32 * step;
        let v0 = row as f32 * step;
        [u0, v0, u0 + step, v0 + step]
    }

    /// พิกัด pixel ของมุมซ้ายบน
    #[must_use]
    pub fn origin_px(self) -> (u32, u32) {
        let per_row = LAYER_SIZE / SLOT_SIZE;
        (
            (self.index % per_row) * SLOT_SIZE,
            (self.index / per_row) * SLOT_SIZE,
        )
    }
}

/// atlas เต็มจนจองช่องใหม่ไม่ได้
#[derive(Debug, thiserror::Error)]
pub enum AtlasError {
    /// ถึงเพดานจำนวน layer แล้ว
    #[error(
        "พื้นที่เก็บภาพย่อเต็ม (ใช้ครบ {layers} ชั้นแล้ว)\nลองปิด board ที่ไม่ได้ใช้ หรือเพิ่มเพดานหน่วยความจำในการตั้งค่า"
    )]
    Full {
        /// จำนวน layer ที่ใช้ไปแล้ว
        layers: u32,
    },
}

/// ตัวจัดสรรช่องใน atlas — free-list ล้วน ไม่แตะ GPU
///
/// แยกจากตัว texture จริงเพื่อให้ **ทดสอบได้โดยไม่ต้องมี GPU**
#[derive(Debug)]
pub struct SlotAllocator {
    max_layers: u32,
    /// จำนวน layer ที่สร้างไปแล้ว
    layers: u32,
    /// ช่องที่คืนมาแล้วพร้อมใช้ซ้ำ
    free: Vec<AtlasSlot>,
    /// ช่องถัดไปที่ยังไม่เคยถูกใช้
    next: u32,
}

impl SlotAllocator {
    /// สร้างตัวจัดสรรที่มีเพดาน layer ตามที่กำหนด
    #[must_use]
    pub fn new(max_layers: u32) -> Self {
        Self {
            max_layers: max_layers.max(1),
            layers: 0,
            free: Vec::new(),
            next: 0,
        }
    }

    /// จองช่องหนึ่งช่อง
    ///
    /// # Errors
    /// คืน [`AtlasError::Full`] เมื่อใช้ครบทุก layer แล้ว
    pub fn allocate(&mut self) -> Result<AtlasSlot, AtlasError> {
        // ใช้ช่องที่คืนมาก่อนเสมอ — ลดจำนวน layer ที่ต้องสร้าง
        if let Some(slot) = self.free.pop() {
            return Ok(slot);
        }

        let layer = self.next / SLOTS_PER_LAYER;
        if layer >= self.max_layers {
            return Err(AtlasError::Full {
                layers: self.max_layers,
            });
        }

        let slot = AtlasSlot {
            layer,
            index: self.next % SLOTS_PER_LAYER,
        };
        self.next += 1;
        self.layers = self.layers.max(layer + 1);
        Ok(slot)
    }

    /// คืนช่องให้ใช้ซ้ำ
    pub fn free(&mut self, slot: AtlasSlot) {
        self.free.push(slot);
    }

    /// จำนวน layer ที่สร้างไปแล้ว
    #[must_use]
    pub fn layers_used(&self) -> u32 {
        self.layers
    }

    /// จำนวนช่องที่ถูกใช้อยู่จริง
    #[must_use]
    pub fn slots_in_use(&self) -> u32 {
        self.next
            - u32::try_from(self.free.len())
                .unwrap_or(u32::MAX)
                .min(self.next)
    }

    /// VRAM ที่ atlas นี้กินอยู่ (ไบต์)
    #[must_use]
    pub fn vram_bytes(&self) -> u64 {
        u64::from(self.layers) * u64::from(LAYER_SIZE) * u64::from(LAYER_SIZE) * 4
    }
}

/// atlas จริงบน GPU
pub struct ThumbnailAtlas {
    /// ถือใบจอง VRAM ไว้ — drop แล้วโควตาคืนเอง
    texture: TrackedTexture,
    view: wgpu::TextureView,
    sampler: wgpu::Sampler,
    bind_group: wgpu::BindGroup,
    bind_group_layout: wgpu::BindGroupLayout,
    allocator: SlotAllocator,
}

impl ThumbnailAtlas {
    /// format ที่ใช้จริง
    ///
    /// sRGB เพื่อให้ GPU แปลง gamma ให้ฟรี (docs/04 §6)
    pub const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8UnormSrgb;

    /// สร้าง atlas ขนาด `layers` ชั้น
    ///
    /// ★ ผูกกับ device — หลัง device lost ต้องสร้างใหม่แล้ว re-upload จาก cache.sqlite
    ///
    /// # Errors
    /// คืน [`VramError`] เมื่อ atlas ขนาดนี้ทะลุเพดาน VRAM
    pub fn new(
        device: &wgpu::Device,
        allocator: &TextureAllocator,
        layers: u32,
    ) -> Result<Self, VramError> {
        let layers = layers.max(1);
        // ★ ผ่าน allocator เท่านั้น ห้ามเรียก device.create_texture() ตรง ๆ
        let texture = allocator.allocate(
            device,
            &wgpu::TextureDescriptor {
                label: Some("refx-thumb-atlas"),
                size: wgpu::Extent3d {
                    width: LAYER_SIZE,
                    height: LAYER_SIZE,
                    depth_or_array_layers: layers,
                },
                mip_level_count: 1, // docs/04 §4: atlas ไม่ต้องมี mip (128px เล็กพอแล้ว)
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: Self::FORMAT,
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            },
        )?;

        let view = texture.create_view(&wgpu::TextureViewDescriptor {
            label: Some("refx-thumb-atlas-view"),
            dimension: Some(wgpu::TextureViewDimension::D2Array),
            ..Default::default()
        });

        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("refx-thumb-sampler"),
            // ClampToEdge: กันสีจากช่องข้าง ๆ รั่วเข้ามาตอน filter ที่ขอบช่อง
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::MipmapFilterMode::Nearest,
            ..Default::default()
        });

        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("refx-atlas-layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2Array,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });

        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("refx-atlas-bind"),
            layout: &bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&sampler),
                },
            ],
        });

        tracing::info!(
            layers,
            vram_mb = (u64::from(layers) * u64::from(LAYER_SIZE) * u64::from(LAYER_SIZE) * 4)
                / (1 << 20),
            format = ?Self::FORMAT,
            "สร้าง thumbnail atlas"
        );

        Ok(Self {
            texture,
            view,
            sampler,
            bind_group,
            bind_group_layout,
            allocator: SlotAllocator::new(layers),
        })
    }

    /// จองช่องแล้วอัปโหลดภาพ 128×128 (RGBA8) ลงไป
    ///
    /// `pixels` ต้องมีความยาว `SLOT_SIZE * SLOT_SIZE * 4` พอดี
    ///
    /// # Errors
    /// คืน [`AtlasError::Full`] เมื่อ atlas เต็ม
    pub fn upload(&mut self, queue: &wgpu::Queue, pixels: &[u8]) -> Result<AtlasSlot, AtlasError> {
        let expected = (SLOT_SIZE * SLOT_SIZE * 4) as usize;
        debug_assert_eq!(pixels.len(), expected, "ขนาด thumbnail ต้องเป็น 128×128 RGBA");
        if pixels.len() != expected {
            // ข้อมูลผิดขนาดต้องไม่ทำให้ write_texture ล้ม — ถือว่า atlas เต็มไปเลย
            tracing::error!(len = pixels.len(), expected, "ขนาด thumbnail ไม่ถูกต้อง");
            return Err(AtlasError::Full {
                layers: self.allocator.layers_used(),
            });
        }

        let slot = self.allocator.allocate()?;
        let (x, y) = slot.origin_px();

        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: self.texture.texture(),
                mip_level: 0,
                origin: wgpu::Origin3d {
                    x,
                    y,
                    z: slot.layer,
                },
                aspect: wgpu::TextureAspect::All,
            },
            pixels,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(SLOT_SIZE * 4),
                rows_per_image: Some(SLOT_SIZE),
            },
            wgpu::Extent3d {
                width: SLOT_SIZE,
                height: SLOT_SIZE,
                depth_or_array_layers: 1,
            },
        );

        Ok(slot)
    }

    /// คืนช่องให้ใช้ซ้ำ (LRU eviction — I-6)
    pub fn free(&mut self, slot: AtlasSlot) {
        self.allocator.free(slot);
    }

    /// bind group สำหรับผูกเข้า render pass
    #[must_use]
    pub fn bind_group(&self) -> &wgpu::BindGroup {
        &self.bind_group
    }

    /// layout ของ bind group (ใช้ตอนสร้าง pipeline)
    #[must_use]
    pub fn bind_group_layout(&self) -> &wgpu::BindGroupLayout {
        &self.bind_group_layout
    }

    /// ตัวจัดสรรช่อง (อ่านสถิติ)
    #[must_use]
    pub fn allocator(&self) -> &SlotAllocator {
        &self.allocator
    }

    /// texture view (เผื่อ debug)
    #[must_use]
    pub fn view(&self) -> &wgpu::TextureView {
        &self.view
    }

    /// sampler ที่ใช้อยู่
    #[must_use]
    pub fn sampler(&self) -> &wgpu::Sampler {
        &self.sampler
    }
}

/// จำนวน layer ที่ควรสร้างตามงบ VRAM และความสามารถของ GPU
///
/// docs/04 §4: 1000 ภาพ = 4 layer (RGBA8 = 64 MB)
#[must_use]
pub fn layers_for_budget(caps: &GpuCapabilities, vram_budget: u64) -> u32 {
    let per_layer = u64::from(LAYER_SIZE) * u64::from(LAYER_SIZE) * 4;
    let by_budget = (vram_budget / per_layer).max(1);
    // การ์ดที่ atlas 2048 ไม่ไหวจะโดนจำกัดด้วย max_texture_dimension_2d อยู่แล้ว
    let by_gpu = if caps.atlas_size() >= LAYER_SIZE {
        16
    } else {
        1
    };
    u32::try_from(by_budget.min(by_gpu)).unwrap_or(1).max(1)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::float_cmp)]

    use super::*;

    #[test]
    fn slots_per_layer_matches_spec() {
        // docs/04 §4: ช่องละ 128×128 → 256 ช่องต่อ layer
        assert_eq!(SLOTS_PER_LAYER, 256);
    }

    #[test]
    fn allocates_sequentially_then_moves_to_next_layer() {
        let mut alloc = SlotAllocator::new(4);
        for i in 0..SLOTS_PER_LAYER {
            let slot = alloc.allocate().unwrap();
            assert_eq!(slot.layer, 0, "256 ช่องแรกต้องอยู่ layer 0");
            assert_eq!(slot.index, i);
        }
        let next = alloc.allocate().unwrap();
        assert_eq!(next.layer, 1, "ช่องที่ 257 ต้องขึ้น layer ใหม่");
        assert_eq!(next.index, 0);
    }

    #[test]
    fn thousand_images_fit_in_four_layers() {
        // ตัวเลขนี้อยู่ใน docs/04 §4 — ถ้าเปลี่ยนต้องแก้ spec ด้วย
        let mut alloc = SlotAllocator::new(8);
        for _ in 0..1000 {
            alloc.allocate().unwrap();
        }
        assert_eq!(alloc.layers_used(), 4);
        assert_eq!(alloc.vram_bytes(), 64 << 20, "RGBA8 4 layer = 64 MB");
    }

    #[test]
    fn freed_slots_are_reused_before_new_ones() {
        let mut alloc = SlotAllocator::new(2);
        let a = alloc.allocate().unwrap();
        let _b = alloc.allocate().unwrap();
        alloc.free(a);

        let reused = alloc.allocate().unwrap();
        assert_eq!(reused, a, "ต้องใช้ช่องที่คืนมาก่อน ไม่ใช่จองช่องใหม่");
    }

    #[test]
    fn full_atlas_reports_error_not_panic() {
        let mut alloc = SlotAllocator::new(1);
        for _ in 0..SLOTS_PER_LAYER {
            alloc.allocate().unwrap();
        }
        let err = alloc.allocate().unwrap_err();
        assert!(matches!(err, AtlasError::Full { layers: 1 }), "ได้ {err:?}");
    }

    /// uv ของแต่ละช่องต้องไม่ทับกันและอยู่ใน [0,1]
    #[test]
    fn uv_rects_are_disjoint_and_in_range() {
        let per_row = LAYER_SIZE / SLOT_SIZE;
        for index in [0u32, 1, per_row, SLOTS_PER_LAYER - 1] {
            let uv = AtlasSlot { layer: 0, index }.uv_rect();
            assert!(
                uv.iter().all(|v| (0.0..=1.0).contains(v)),
                "uv นอกช่วง: {uv:?}"
            );
            assert!(uv[2] > uv[0] && uv[3] > uv[1], "uv กลับด้าน: {uv:?}");
        }

        // ช่อง 0 กับช่อง 1 ต้องอยู่คนละที่
        let a = AtlasSlot { layer: 0, index: 0 }.uv_rect();
        let b = AtlasSlot { layer: 0, index: 1 }.uv_rect();
        assert_eq!(a[2], b[0], "ช่องติดกันต้องต่อกันพอดี");
    }

    #[test]
    fn origin_px_matches_uv() {
        let slot = AtlasSlot {
            layer: 2,
            index: 17,
        };
        let (x, y) = slot.origin_px();
        let uv = slot.uv_rect();
        assert_eq!(x as f32 / LAYER_SIZE as f32, uv[0]);
        assert_eq!(y as f32 / LAYER_SIZE as f32, uv[1]);
    }

    #[test]
    fn slots_in_use_tracks_free_list() {
        let mut alloc = SlotAllocator::new(2);
        let a = alloc.allocate().unwrap();
        alloc.allocate().unwrap();
        assert_eq!(alloc.slots_in_use(), 2);
        alloc.free(a);
        assert_eq!(alloc.slots_in_use(), 1);
    }

    #[test]
    fn layers_for_budget_respects_vram() {
        let caps = GpuCapabilities {
            adapter_name: "test".to_owned(),
            backend: wgpu::Backend::Noop,
            device_type: wgpu::DeviceType::Cpu,
            bc_compression: false,
            max_texture_dimension_2d: 8192,
        };
        // งบ 64 MB → 4 layer พอดี
        assert_eq!(layers_for_budget(&caps, 64 << 20), 4);
        // งบเล็กมากต้องยังได้อย่างน้อย 1 layer ไม่ใช่ 0
        assert_eq!(layers_for_budget(&caps, 1024), 1);
    }
}
