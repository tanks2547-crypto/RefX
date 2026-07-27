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

    /// ยังไม่ถึงเพดาน layer แต่จอง layer ใหม่ไม่ได้เพราะ VRAM ไม่พอ
    ///
    /// แยกจาก [`AtlasError::Full`] เพราะสาเหตุและสิ่งที่ผู้ใช้ทำได้ต่างกัน —
    /// ข้อความจริงมาจาก [`VramError`] ซึ่งบอกตัวเลขที่ใช้อยู่/เพดานให้แล้ว
    #[error(transparent)]
    OutOfVram(#[from] VramError),
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

    /// layer ที่การจองครั้งถัดไปจะไปลง — **ไม่เปลี่ยนสถานะ**
    ///
    /// `None` = ไม่มีที่เหลือแล้ว
    ///
    /// ★ มีไว้ให้ [`ThumbnailAtlas`] รู้ล่วงหน้าว่าต้องขยาย texture ไหม **ก่อน**
    /// จะจองช่องจริง ถ้าถามทีหลังแล้วขยายไม่สำเร็จ จะต้องคืนช่องที่จองไปแล้ว
    /// ซึ่งทำให้ตัวนับ `layers` เพี้ยนค้างไว้โดยไม่มีทางแก้กลับ
    #[must_use]
    pub fn next_layer(&self) -> Option<u32> {
        // ช่องที่คืนมาแล้วอยู่ใน layer ที่จองไว้แล้วเสมอ จึงไม่ต้องขยาย
        if let Some(slot) = self.free.last() {
            return Some(slot.layer);
        }
        let layer = self.next / SLOTS_PER_LAYER;
        (layer < self.max_layers).then_some(layer)
    }

    /// เพดานจำนวน layer ของ atlas นี้
    #[must_use]
    pub fn max_layers(&self) -> u32 {
        self.max_layers
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
///
/// ★ **จอง layer แบบ lazy** (ตัดสิน 28 ก.ค. 2026, docs/05 §2)
/// ก่อนหน้านี้จองเต็มเพดาน (12 layer = 192 MB) ตั้งแต่เปิดโปรแกรมแม้ยังไม่มีภาพสักใบ
/// ซึ่งผิดหลักโดเมนตรง ๆ: RefX ถูกเปิดค้างทั้งวันข้าง Photoshop การยึด VRAM ไว้เฉย ๆ
/// คือการแย่ง VRAM จากโปรแกรมหลักของผู้ใช้ บน iGPU ยิ่งหนักเพราะเป็น RAM ระบบ
pub struct ThumbnailAtlas {
    /// ถือใบจอง VRAM ไว้ — drop แล้วโควตาคืนเอง
    ///
    /// ตอน `layers == 0` ตัวนี้คือ texture 1×1 (4 ไบต์) ที่มีไว้ให้ bind group
    /// มีของผูกอยู่เท่านั้น ไม่ได้เก็บภาพอะไร
    texture: TrackedTexture,
    /// จำนวน layer ขนาดเต็มที่จองจริงบน GPU แล้ว — `0` = ยังไม่มีภาพเลย
    layers: u32,
    view: wgpu::TextureView,
    sampler: wgpu::Sampler,
    bind_group: wgpu::BindGroup,
    bind_group_layout: wgpu::BindGroupLayout,
    allocator: SlotAllocator,
    /// ทางเดียวที่ atlas จอง/คืน VRAM ได้ (I-6) — ถือไว้เพราะต้องจองเพิ่มระหว่างทาง
    textures: TextureAllocator,
}

/// descriptor ของ texture atlas ที่มี `layers` ชั้นขนาดเต็ม
///
/// `layers == 0` คืน texture 1×1 แทน เพราะ (ก) wgpu ไม่ยอมให้สร้าง texture
/// ที่มี 0 layer และ (ข) bind group ต้องมีของจริงผูกอยู่เสมอ ไม่งั้นต้องทำ
/// `Option<BindGroup>` แล้วลามไปทั้งเส้นทางวาด — 4 ไบต์ถูกกว่ามาก
fn atlas_descriptor(layers: u32) -> wgpu::TextureDescriptor<'static> {
    let side = if layers == 0 { 1 } else { LAYER_SIZE };
    wgpu::TextureDescriptor {
        label: Some("refx-thumb-atlas"),
        size: wgpu::Extent3d {
            width: side,
            height: side,
            depth_or_array_layers: layers.max(1),
        },
        mip_level_count: 1, // docs/04 §4: atlas ไม่ต้องมี mip (128px เล็กพอแล้ว)
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: ThumbnailAtlas::FORMAT,
        // COPY_SRC จำเป็นตอนขยาย — ต้องคัดลอก layer เดิมไปยัง texture ใบใหม่
        usage: wgpu::TextureUsages::TEXTURE_BINDING
            | wgpu::TextureUsages::COPY_DST
            | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    }
}

impl ThumbnailAtlas {
    /// format ที่ใช้จริง
    ///
    /// sRGB เพื่อให้ GPU แปลง gamma ให้ฟรี (docs/04 §6)
    pub const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8UnormSrgb;

    /// สร้าง atlas ที่มีเพดาน `max_layers` ชั้น — **ยังไม่จอง VRAM ให้ layer ไหนเลย**
    ///
    /// `max_layers` คือ *เพดาน* ไม่ใช่จำนวนที่จองทันที layer จริงถูกจองทีละชั้น
    /// เมื่อภาพล้นชั้นเดิม (ดู [`ThumbnailAtlas::upload`])
    ///
    /// ★ ผูกกับ device — หลัง device lost ต้องสร้างใหม่แล้ว re-upload จาก cache.sqlite
    ///
    /// # Errors
    /// คืน [`VramError`] เมื่อจอง texture เปล่า 1×1 ยังไม่ได้ (เพดาน VRAM เล็กผิดปกติ)
    pub fn new(
        device: &wgpu::Device,
        allocator: &TextureAllocator,
        max_layers: u32,
    ) -> Result<Self, VramError> {
        let max_layers = max_layers.max(1);
        // ★ ผ่าน allocator เท่านั้น ห้ามเรียก device.create_texture() ตรง ๆ
        let texture = allocator.allocate(device, &atlas_descriptor(0))?;
        let view = Self::make_view(&texture);

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

        let bind_group = Self::make_bind_group(device, &bind_group_layout, &view, &sampler);

        tracing::info!(
            max_layers,
            vram_bytes = texture.bytes(),
            format = ?Self::FORMAT,
            "สร้าง thumbnail atlas (ยังไม่จอง layer — จองทีละชั้นตอนมีภาพจริง)"
        );

        Ok(Self {
            texture,
            layers: 0,
            view,
            sampler,
            bind_group,
            bind_group_layout,
            allocator: SlotAllocator::new(max_layers),
            textures: allocator.clone(),
        })
    }

    /// view แบบ `D2Array` ของ texture ที่ให้มา
    fn make_view(texture: &TrackedTexture) -> wgpu::TextureView {
        texture.create_view(&wgpu::TextureViewDescriptor {
            label: Some("refx-thumb-atlas-view"),
            dimension: Some(wgpu::TextureViewDimension::D2Array),
            ..Default::default()
        })
    }

    /// bind group ที่ผูก view + sampler เข้ากับ layout เดิม
    ///
    /// layout ไม่เปลี่ยนตอนขยาย atlas จึงไม่ต้องสร้าง pipeline ใหม่ตาม
    fn make_bind_group(
        device: &wgpu::Device,
        layout: &wgpu::BindGroupLayout,
        view: &wgpu::TextureView,
        sampler: &wgpu::Sampler,
    ) -> wgpu::BindGroup {
        device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("refx-atlas-bind"),
            layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(sampler),
                },
            ],
        })
    }

    /// ขยาย atlas ให้มี `want` layer แล้วย้ายภาพเดิมตามไป
    ///
    /// ★ ต้องคัดลอกของเดิมเสมอ ถ้าปล่อยให้หาย board จะว่างเปล่าทันทีที่ภาพที่ 257
    /// ถูกเพิ่มเข้ามา ซึ่งผู้ใช้แยกไม่ออกจาก "งานหาย" (เหตุผลเดียวกับ §4 ข้อ 5)
    /// การคัดลอกเป็น GPU→GPU 16 MB ต่อชั้น และเกิดอย่างมาก `max_layers - 1` ครั้ง
    /// ตลอดอายุ board — ถูกกว่าการยึด VRAM ไว้ล่วงหน้าทั้งวันมาก
    ///
    /// ระหว่างคัดลอกต้องถือ texture สองใบพร้อมกัน (เดิม + ใหม่) ซึ่งเป็นจุดที่กิน
    /// VRAM สูงสุด — ถ้าเพดานไม่พอช่วงนั้น จะได้ [`VramError`] กลับไปตามปกติ
    /// แล้วภาพนั้นขึ้นเป็น placeholder สีเด่นแทน ไม่ใช่ crash
    fn grow(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        want: u32,
    ) -> Result<(), VramError> {
        debug_assert!(want > self.layers, "grow() ต้องถูกเรียกเมื่อต้องโตขึ้นเท่านั้น");
        let want = want.min(self.allocator.max_layers());
        let next = self.textures.allocate(device, &atlas_descriptor(want))?;

        if self.layers > 0 {
            let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("refx-atlas-grow"),
            });
            encoder.copy_texture_to_texture(
                self.texture.texture().as_image_copy(),
                next.texture().as_image_copy(),
                wgpu::Extent3d {
                    width: LAYER_SIZE,
                    height: LAYER_SIZE,
                    depth_or_array_layers: self.layers,
                },
            );
            queue.submit(std::iter::once(encoder.finish()));
        }

        // ใบเก่าถูก drop ตรงนี้ → โควตา VRAM คืนเอง (RAII)
        // wgpu ถือ texture ไว้จนคำสั่งคัดลอกทำงานจบ จึงปล่อยได้เลยไม่ต้องรอ
        self.texture = next;
        self.layers = want;
        self.view = Self::make_view(&self.texture);
        self.bind_group =
            Self::make_bind_group(device, &self.bind_group_layout, &self.view, &self.sampler);

        tracing::debug!(
            layers = self.layers,
            vram_bytes = self.texture.bytes(),
            "ขยาย thumbnail atlas"
        );
        Ok(())
    }

    /// จำนวน layer ที่จอง VRAM จริงแล้ว — `0` ตอนเปิดโปรแกรมเปล่า
    #[must_use]
    pub fn layers_allocated(&self) -> u32 {
        self.layers
    }

    /// VRAM ที่ atlas นี้ถืออยู่จริง (ไบต์)
    #[must_use]
    pub fn vram_bytes(&self) -> usize {
        self.texture.bytes()
    }

    /// จองช่องแล้วอัปโหลดภาพ 128×128 (RGBA8) ลงไป
    ///
    /// `pixels` ต้องมีความยาว `SLOT_SIZE * SLOT_SIZE * 4` พอดี
    ///
    /// จอง layer เพิ่มให้เองเมื่อภาพล้นชั้นเดิม — ผู้เรียกไม่ต้องรู้เรื่องนี้
    ///
    /// # Errors
    /// คืน [`AtlasError::Full`] เมื่อใช้ครบเพดาน layer แล้ว หรือ
    /// [`AtlasError::OutOfVram`] เมื่อยังไม่ถึงเพดานแต่จอง layer ใหม่ไม่ได้
    pub fn upload(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        pixels: &[u8],
    ) -> Result<AtlasSlot, AtlasError> {
        let expected = (SLOT_SIZE * SLOT_SIZE * 4) as usize;
        debug_assert_eq!(pixels.len(), expected, "ขนาด thumbnail ต้องเป็น 128×128 RGBA");
        if pixels.len() != expected {
            // ข้อมูลผิดขนาดต้องไม่ทำให้ write_texture ล้ม — ถือว่า atlas เต็มไปเลย
            tracing::error!(len = pixels.len(), expected, "ขนาด thumbnail ไม่ถูกต้อง");
            return Err(AtlasError::Full {
                layers: self.allocator.layers_used(),
            });
        }

        // ★ ถามก่อนจอง: ต้องรู้ว่าช่องถัดไปอยู่ layer ไหนเพื่อขยาย texture ให้พอ
        //   **ก่อน** ที่ช่องจะถูกจองจริง ถ้าขยายทีหลังแล้วไม่สำเร็จ จะต้องคืนช่อง
        //   ซึ่งทำให้ตัวนับ layer เพี้ยนค้าง
        let next_layer = self.allocator.next_layer().ok_or(AtlasError::Full {
            layers: self.allocator.max_layers(),
        })?;
        if next_layer >= self.layers {
            self.grow(device, queue, next_layer + 1)?;
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

/// **เพดาน** จำนวน layer ตามงบ VRAM และความสามารถของ GPU
///
/// ★ เป็นเพดานเท่านั้น ไม่ใช่จำนวนที่จองทันที — [`ThumbnailAtlas`] จองทีละชั้น
/// ตามการใช้จริง (docs/05 §2)
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

    // ---------- lazy allocation (2.2) ----------
    //
    // ทดสอบที่ `SlotAllocator` เพราะเป็นตัวตัดสินว่าต้องขยาย texture เมื่อไหร่
    // และทดสอบได้โดยไม่ต้องมี GPU — `ThumbnailAtlas::grow()` แค่ทำตามคำตอบนี้

    /// ★ ข้อกำหนดหลักของ 2.2: ยังไม่มีภาพ = ยังไม่ต้องมี layer สักชั้น
    #[test]
    fn empty_atlas_needs_no_layer() {
        let alloc = SlotAllocator::new(12);
        assert_eq!(alloc.layers_used(), 0, "ยังไม่มีภาพต้องไม่ใช้ layer เลย");
        assert_eq!(alloc.vram_bytes(), 0, "ยังไม่มีภาพต้องไม่กิน VRAM เลย");
        // แต่ยังต้องบอกได้ว่าภาพแรกจะไปลง layer ไหน
        assert_eq!(alloc.next_layer(), Some(0));
    }

    /// ★ จำนวน layer ต้องโตตามภาพจริง ไม่ใช่กระโดดไปเต็มเพดาน
    #[test]
    fn layers_grow_one_at_a_time_with_real_use() {
        let mut alloc = SlotAllocator::new(12);
        for n in 1..=(SLOTS_PER_LAYER * 3) {
            alloc.allocate().unwrap();
            let expected = n.div_ceil(SLOTS_PER_LAYER);
            assert_eq!(
                alloc.layers_used(),
                expected,
                "ภาพที่ {n} ควรใช้ {expected} layer"
            );
        }
        // 3 layer = 48 MB ไม่ใช่ 192 MB ของเพดาน 12 ชั้น
        assert_eq!(alloc.vram_bytes(), 48 << 20);
    }

    /// `next_layer()` ต้องตรงกับ layer ที่ `allocate()` คืนจริงเสมอ
    ///
    /// ถ้าสองอันนี้ไม่ตรงกัน atlas จะขยายผิดชั้นแล้ว `write_texture` ยิงนอกขอบเขต
    #[test]
    fn next_layer_matches_what_allocate_returns() {
        let mut alloc = SlotAllocator::new(4);
        for _ in 0..(SLOTS_PER_LAYER * 2 + 5) {
            let predicted = alloc.next_layer().expect("ยังไม่เต็ม");
            let slot = alloc.allocate().unwrap();
            assert_eq!(predicted, slot.layer);
        }
    }

    /// ช่องที่คืนมาแล้วอยู่ใน layer ที่จองไว้แล้ว → ต้องไม่สั่งขยาย atlas ซ้ำ
    #[test]
    fn reused_slot_never_asks_for_a_new_layer() {
        let mut alloc = SlotAllocator::new(4);
        let first = alloc.allocate().unwrap();
        alloc.free(first);
        assert_eq!(
            alloc.next_layer(),
            Some(first.layer),
            "ช่องที่คืนมาต้องไม่ทำให้ atlas โตขึ้น"
        );
        assert_eq!(alloc.allocate().unwrap(), first);
    }

    /// เต็มเพดานแล้วต้องตอบ `None` ไม่ใช่ชี้ไป layer ที่ไม่มีอยู่จริง
    #[test]
    fn next_layer_is_none_when_full() {
        let mut alloc = SlotAllocator::new(1);
        for _ in 0..SLOTS_PER_LAYER {
            alloc.allocate().unwrap();
        }
        assert_eq!(alloc.next_layer(), None);
        assert_eq!(alloc.max_layers(), 1);
    }

    /// ★ เทียบตรง ๆ กับพฤติกรรมเดิม: เพดาน 12 ชั้นต้องไม่แปลว่าจอง 192 MB
    #[test]
    fn cap_of_twelve_layers_costs_nothing_until_used() {
        let caps = GpuCapabilities {
            adapter_name: "test".to_owned(),
            backend: wgpu::Backend::Noop,
            device_type: wgpu::DeviceType::DiscreteGpu,
            bc_compression: false,
            max_texture_dimension_2d: 8192,
        };
        // ครึ่งงบของ dGPU (384/2 = 192 MB) → เพดาน 12 ชั้นเหมือนเดิม
        let max_layers = layers_for_budget(&caps, (384 << 20) / 2);
        assert_eq!(max_layers, 12);

        let mut alloc = SlotAllocator::new(max_layers);
        assert_eq!(alloc.vram_bytes(), 0, "เพดาน 12 ชั้นต้องยังไม่กิน VRAM");
        alloc.allocate().unwrap();
        assert_eq!(alloc.vram_bytes(), 16 << 20, "ภาพแรกจอง 1 ชั้น = 16 MB");
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
