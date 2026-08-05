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
    #[error("thumbnail atlas is full ({layers} layers all in use)")]
    Full {
        /// จำนวน layer ที่ใช้ไปแล้ว
        layers: u32,
    },

    /// ยังมีที่เหลือตามเพดาน แต่ texture ปัจจุบันเล็กเกินไป
    ///
    /// ★ เป็น **สัญญาณควบคุมภายใน** ไม่ใช่ความล้มเหลว — ผู้เรียกต้องเรียก
    /// [`ThumbnailAtlas::resize`] แล้วอัปโหลดภาพเดิมกลับทั้งหมดก่อนลองใหม่
    /// (ดูเหตุผลที่ไม่ขยายให้เองใน `resize`)
    ///
    /// ถ้าข้อความนี้ไปโผล่ให้ผู้ใช้เห็น แปลว่าชั้น UI ลืมจัดการ
    #[error("atlas needs {layers} layers but only has room for fewer")]
    NeedsResize {
        /// จำนวน layer ที่ต้องมีเพื่อรับภาพถัดไป
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

    /// ล้างการจองทั้งหมด กลับไปเหมือนเพิ่งสร้าง
    ///
    /// ใช้ตอน atlas ถูกสร้าง texture ใหม่ทั้งใบ (ขยายขนาด หรือกู้ device)
    /// — ภาพเดิมหายไปกับ texture เก่าแล้ว ช่องที่เคยจองไว้จึงไม่มีความหมายอีก
    /// ผู้เรียกต้องอัปโหลดภาพเดิมกลับเข้ามาใหม่ทั้งหมด
    pub fn reset(&mut self) {
        self.layers = 0;
        self.free.clear();
        self.next = 0;
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

/// ต้องมีกี่ layer ถึงจะเก็บภาพ `slots` ภาพได้
///
/// ★ มีไว้ให้ **เติม atlas กลับหลังกู้ device** ขยาย texture ให้พอ *ก่อน* เริ่มเติม
///
/// ทำไมต้องขยายก่อน ไม่ใช่ขยายตอนเจอ `NeedsResize` กลางทาง:
/// [`ThumbnailAtlas::resize`] สร้าง texture ใบใหม่แล้ว **ล้างตัวจัดสรรทั้งหมด**
/// ช่องที่แจกไปแล้วในรอบเดียวกันจะชี้ไปที่ texture ที่ถูกทิ้งไปแล้วทันที
///
/// เจอของจริง 29 ก.ค. 2026: atlas ที่เพิ่งสร้างมี 0 layer (จองแบบ lazy — docs/05 §2)
/// การเติมกลับจึงได้ `NeedsResize` ตั้งแต่ภาพแรกแล้ว **ทุกภาพกลายเป็น placeholder**
/// = board ว่างเปล่าหลัง driver อัปเดต ซึ่งคือสิ่งที่ข้อผูกมัดเรื่องเติม atlas กลับ
/// (docs/04 §4) มีไว้กันพอดี
#[must_use]
pub fn layers_needed(slots: usize) -> u32 {
    let per_layer = SLOTS_PER_LAYER as usize;
    // ปัดขึ้นเสมอ — เหลือเศษหนึ่งภาพก็ต้องมี layer ให้มันอยู่
    let layers = slots.div_ceil(per_layer);
    u32::try_from(layers).unwrap_or(u32::MAX).max(1)
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
    /// ★ จำนวนครั้งที่เขียน pixel ลง texture จริง ๆ ตลอดอายุ atlas
    ///
    /// มีไว้พิสูจน์เกณฑ์ของ ROADMAP P2-8: **สลับ grayscale ทั้ง board = 0 texture upload**
    /// ตัวเลขที่ไม่มีใครนับคือคำกล่าวอ้าง ไม่ใช่หลักฐาน (docs/08 §3.9 ข้อ 6)
    uploads: u64,
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
            "thumbnail atlas created (no layer reserved yet — layers are allocated lazily)"
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
            uploads: 0,
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

    /// สร้าง texture ใหม่ให้มี `layers` ชั้น — **ภาพเดิมหายทั้งหมด**
    ///
    /// ผู้เรียกต้องอัปโหลดภาพเดิมกลับเข้ามาใหม่ทันทีหลังเรียก (ดู `refill_atlas`
    /// ใน `refx-ui` ซึ่งทำงานนี้อยู่แล้วสำหรับเส้นทางกู้ device)
    ///
    /// ★ ทำไมไม่คัดลอกของเดิมด้วย GPU แล้วขยายให้เองเงียบ ๆ (แบบเดิม):
    ///
    /// การคัดลอกบังคับให้ **ถือ texture สองใบพร้อมกัน** ตอนขยายจาก 11 → 12 layer
    /// นั่นคือ 176 + 192 = **368 MB จากเพดาน 384 MB** ซึ่งพอ P1-7 มาใช้อีกครึ่ง
    /// จะจองไม่ผ่านแล้วภาพหลังจากนั้นกลายเป็น placeholder ทั้งหมด
    ///
    /// วิธีนี้ปล่อยใบเก่า **ก่อน** จองใบใหม่ → peak = ขนาดใบใหม่ใบเดียว (192 MB)
    /// ต้นทุนที่แลกมาคืออัปโหลดภาพเดิมกลับจาก RAM ซึ่งวัดแล้วเร็วมาก
    /// (100 ภาพใน 0.99 ms) และเกิดอย่างมาก `max_layers - 1` ครั้งตลอดอายุ board
    /// ภาพย่อทุกใบถูกเก็บใน RAM อยู่แล้วเพื่อเส้นทางกู้ device จึงไม่ต้องอ่านดิสก์ซ้ำ
    ///
    /// # Errors
    /// คืน [`VramError`] เมื่อจอง texture ขนาดใหม่ไม่ได้ — ในกรณีนั้นจะพยายาม
    /// ถอยกลับไปขนาดเดิมให้ เพื่อให้ผู้เรียกเติมภาพเดิมกลับได้เท่าที่เคยมี
    pub fn resize(&mut self, device: &wgpu::Device, layers: u32) -> Result<(), VramError> {
        let want = layers.clamp(1, self.allocator.max_layers());
        let previous = self.layers;
        let before = self.textures.budget().used();

        // ★ หัวใจของการแก้อยู่ที่ลำดับสองบรรทัดนี้
        //   จอง placeholder 1×1 (4 ไบต์) แล้วเขียนทับ `self.texture`
        //   → ใบเก่าถูก drop ทันที คืนโควตาก่อนที่เราจะขอใบใหม่
        let placeholder = self.textures.allocate(device, &atlas_descriptor(0))?;
        self.texture = placeholder;
        self.layers = 0;
        self.allocator.reset();
        let after_release = self.textures.budget().used();

        let fresh = match self.textures.allocate(device, &atlas_descriptor(want)) {
            Ok(texture) => texture,
            Err(err) => {
                // ถอยกลับไปขนาดเดิม — เพิ่งคืนโควตาขนาดนั้นไป จึงควรจองคืนได้
                // ผู้เรียกจะได้เติมภาพเดิมกลับได้ครบเท่าที่เคยมี ไม่ใช่ board ว่างเปล่า
                if previous > 0
                    && let Ok(same) = self.textures.allocate(device, &atlas_descriptor(previous))
                {
                    self.texture = same;
                    self.layers = previous;
                    self.rebuild_bindings(device);
                }
                tracing::warn!(%err, want, previous, "cannot grow the atlas — falling back to the previous size");
                return Err(err);
            }
        };

        self.texture = fresh;
        self.layers = want;
        self.rebuild_bindings(device);

        tracing::debug!(
            layers = self.layers,
            vram_before = before,
            vram_after_release = after_release,
            vram_after = self.textures.budget().used(),
            "atlas texture recreated (old one released first — never holding two at once)"
        );
        Ok(())
    }

    /// สร้าง view + bind group ใหม่หลังเปลี่ยน texture
    ///
    /// layout ไม่เปลี่ยน จึงไม่ต้องสร้าง pipeline ใหม่ตาม
    fn rebuild_bindings(&mut self, device: &wgpu::Device) {
        self.view = Self::make_view(&self.texture);
        self.bind_group =
            Self::make_bind_group(device, &self.bind_group_layout, &self.view, &self.sampler);
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
    /// **ไม่ขยาย texture ให้เอง** — ถ้าที่ไม่พอจะคืน [`AtlasError::NeedsResize`]
    /// ผู้เรียกต้องเรียก [`ThumbnailAtlas::resize`] แล้วอัปโหลดภาพเดิมกลับก่อนลองใหม่
    /// (เหตุผลอยู่ใน `resize` — การขยายเองบังคับให้ถือ texture สองใบพร้อมกัน)
    ///
    /// # Errors
    /// [`AtlasError::Full`] เมื่อใช้ครบเพดาน layer แล้ว ·
    /// [`AtlasError::NeedsResize`] เมื่อยังมีที่ตามเพดานแต่ texture เล็กเกินไป
    pub fn upload(&mut self, queue: &wgpu::Queue, pixels: &[u8]) -> Result<AtlasSlot, AtlasError> {
        let expected = (SLOT_SIZE * SLOT_SIZE * 4) as usize;
        debug_assert_eq!(pixels.len(), expected, "ขนาด thumbnail ต้องเป็น 128×128 RGBA");
        if pixels.len() != expected {
            // ข้อมูลผิดขนาดต้องไม่ทำให้ write_texture ล้ม — ถือว่า atlas เต็มไปเลย
            tracing::error!(
                len = pixels.len(),
                expected,
                "thumbnail has the wrong byte length"
            );
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
            return Err(AtlasError::NeedsResize {
                layers: next_layer + 1,
            });
        }

        let slot = self.allocator.allocate()?;
        let (x, y) = slot.origin_px();

        // นับ **ก่อน** เขียนจริง — ตัวเลขนี้คือหลักฐานของเกณฑ์ "0 texture upload"
        self.uploads += 1;
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

    /// จำนวนครั้งที่อัป pixel ขึ้น texture ตลอดอายุ atlas (ดูฟิลด์ `uploads`)
    #[must_use]
    pub fn uploads(&self) -> u64 {
        self.uploads
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
    // เทสต์ต้อง panic! ได้เมื่อผลไม่ตรงชนิดที่คาด — "ล้มเหลวด้วยเหตุผลที่ถูก" สำคัญพอกัน
    #![allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::float_cmp,
        clippy::panic
    )]

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

    /// ★ หลัง `reset` ต้องเริ่มนับใหม่จากศูนย์ทั้งหมด
    ///
    /// `resize` สร้าง texture ใหม่ที่ว่างเปล่า ภาพเดิมหายไปกับใบเก่า
    /// ถ้าตัวจัดสรรช่องไม่ถูกล้างด้วย ช่องที่ "จองแล้ว" จะชี้ไปยังพื้นที่ที่ไม่มีภาพ
    /// แล้ว board จะขึ้นเป็นช่องดำ ๆ แทนภาพ โดยที่ทุกอย่างดู "สำเร็จ" หมด
    #[test]
    fn reset_makes_the_allocator_start_over() {
        let mut alloc = SlotAllocator::new(12);
        for _ in 0..(SLOTS_PER_LAYER + 5) {
            alloc.allocate().unwrap();
        }
        alloc.free(AtlasSlot { layer: 0, index: 3 });
        assert_eq!(alloc.layers_used(), 2);

        alloc.reset();

        assert_eq!(alloc.layers_used(), 0);
        assert_eq!(alloc.slots_in_use(), 0);
        assert_eq!(alloc.vram_bytes(), 0);
        assert_eq!(alloc.next_layer(), Some(0));
        // ช่องแรกหลัง reset ต้องเป็นช่องแรกจริง ๆ ไม่ใช่ช่องที่ค้างอยู่ใน free list
        assert_eq!(alloc.allocate().unwrap(), AtlasSlot { layer: 0, index: 0 });
        // เพดานต้องไม่หายไปกับการ reset
        assert_eq!(alloc.max_layers(), 12);
    }

    /// เติมภาพกลับหลัง reset ต้องได้ลำดับช่องเหมือนเดิมเป๊ะ
    ///
    /// `refill_atlas` เดินตาม `board_thumbs` ตามลำดับแล้วเขียนทับ `quads[i]`
    /// ถ้าลำดับช่องไม่ตรงกับรอบแรก ภาพจะสลับที่กันทั้ง board
    #[test]
    fn refilling_after_reset_reproduces_the_same_slots() {
        let mut alloc = SlotAllocator::new(12);
        let first: Vec<AtlasSlot> = (0..600).map(|_| alloc.allocate().unwrap()).collect();

        alloc.reset();
        let second: Vec<AtlasSlot> = (0..600).map(|_| alloc.allocate().unwrap()).collect();

        assert_eq!(first, second);
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

    // ---------- ★ GPU จริง: เติม atlas กลับหลัง device lost (P0-5) ----------
    //
    // เทสต์กลุ่มนี้ต้องมี GPU จริง เพราะสิ่งที่ต้องพิสูจน์คือ **pixel อยู่บน texture
    // ของ device ใบใหม่จริง ๆ** ไม่ใช่แค่ตัวนับ slot ตรงกัน — ตัวนับตรงได้ทั้งที่
    // ภาพไม่ได้ขึ้น ซึ่งเป็นบั๊ก "canvas ว่างเปล่า" แบบเดียวกับที่โปรเจกต์นี้เคยโดน
    //
    // เครื่องที่ไม่มี GPU จะ **ข้ามพร้อมพิมพ์เหตุผล** (docs/08 §3.9 ข้อ 2)

    /// สีทึบขนาดเท่าช่อง atlas พอดี
    fn solid_thumb(rgba: [u8; 4]) -> Vec<u8> {
        rgba.iter()
            .copied()
            .cycle()
            .take((SLOT_SIZE * SLOT_SIZE * 4) as usize)
            .collect()
    }

    /// อ่าน pixel ของช่องหนึ่งกลับจาก GPU — **หลักฐานว่าภาพขึ้นจริง**
    fn read_slot(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        atlas: &ThumbnailAtlas,
        slot: AtlasSlot,
    ) -> Vec<u8> {
        let (x, y) = slot.origin_px();
        // 128 × 4 = 512 ไบต์ต่อแถว ซึ่งหาร 256 ลงตัวพอดีตามที่ wgpu บังคับ
        let bytes_per_row = SLOT_SIZE * 4;
        let size = u64::from(bytes_per_row) * u64::from(SLOT_SIZE);

        let buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("refx-test-readback"),
            size,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });

        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("refx-test-readback"),
        });
        encoder.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture: atlas.texture.texture(),
                mip_level: 0,
                origin: wgpu::Origin3d {
                    x,
                    y,
                    z: slot.layer,
                },
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyBufferInfo {
                buffer: &buffer,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(bytes_per_row),
                    rows_per_image: Some(SLOT_SIZE),
                },
            },
            wgpu::Extent3d {
                width: SLOT_SIZE,
                height: SLOT_SIZE,
                depth_or_array_layers: 1,
            },
        );
        queue.submit(std::iter::once(encoder.finish()));

        buffer.slice(..).map_async(wgpu::MapMode::Read, |_| {});
        // ★ ต้องมี timeout เสมอ — เทสต์ที่ค้างตลอดกาลใน CI แย่กว่าเทสต์ที่ล้ม
        device
            .poll(wgpu::PollType::Wait {
                submission_index: None,
                timeout: Some(std::time::Duration::from_secs(10)),
            })
            .expect("รอ GPU ไม่สำเร็จ");
        let pixels = buffer.slice(..).get_mapped_range().to_vec();
        buffer.unmap();
        pixels
    }

    /// สร้าง atlas พร้อมใช้บน device ที่ให้มา
    fn fresh_atlas(device: &wgpu::Device) -> (TextureAllocator, ThumbnailAtlas) {
        let textures = TextureAllocator::with_limit(128 << 20);
        let atlas = ThumbnailAtlas::new(device, &textures, 4).expect("สร้าง atlas ไม่ได้");
        (textures, atlas)
    }

    /// อัปโหลดชุดภาพเดียวกันตามลำดับ — เลียนแบบสิ่งที่ `refill_atlas` ทำเป๊ะ
    fn upload_all(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        atlas: &mut ThumbnailAtlas,
        thumbs: &[Vec<u8>],
    ) -> Vec<AtlasSlot> {
        thumbs
            .iter()
            .map(|pixels| match atlas.upload(queue, pixels) {
                Ok(slot) => slot,
                Err(AtlasError::NeedsResize { layers }) => {
                    atlas.resize(device, layers).expect("ขยาย atlas ไม่ได้");
                    atlas.upload(queue, pixels).expect("อัปโหลดหลังขยายไม่ได้")
                }
                Err(err) => panic!("อัปโหลดไม่สำเร็จ: {err}"),
            })
            .collect()
    }

    /// ★ หัวใจของ P0-5: device ตายทั้งใบ → สร้างใหม่ → **ภาพต้องกลับมาครบ**
    ///
    /// ทิ้ง device ใบเก่าทั้งก้อน (เหมือน `RenderContext::recover`) แล้วสร้างใหม่
    /// จากนั้นเติมภาพจาก RAM กลับขึ้น atlas ใบใหม่ แล้ว **อ่าน pixel กลับมาเทียบ**
    ///
    /// ถ้าไม่เติมกลับ ผู้ใช้จะเห็น board ว่างเปล่าหลัง driver อัปเดต
    /// ซึ่งจากมุมเขาแยกไม่ออกจาก "งานหาย" (docs/04 §4)
    #[test]
    fn atlas_comes_back_with_the_same_pixels_on_a_brand_new_device() {
        let Some((device, queue, _)) = crate::device::gpu_for_test() else {
            return; // ข้าม/ล้ม ถูกตัดสินที่ gpu_for_test แล้ว (docs/08 §3.9 ข้อ 7)
        };

        let thumbs = vec![
            solid_thumb([200, 30, 40, 255]),
            solid_thumb([30, 200, 40, 255]),
            solid_thumb([30, 40, 200, 255]),
        ];

        // ---- device ใบแรก ----
        let before: Vec<Vec<u8>> = {
            let (_textures, mut atlas) = fresh_atlas(&device);
            let slots = upload_all(&device, &queue, &mut atlas, &thumbs);
            let read: Vec<Vec<u8>> = slots
                .iter()
                .map(|&slot| read_slot(&device, &queue, &atlas, slot))
                .collect();
            // ยืนยันว่ารอบแรกภาพขึ้นจริงก่อน — ไม่งั้นเทียบ "ว่างกับว่าง" ก็ผ่าน
            for (i, pixels) in read.iter().enumerate() {
                assert_eq!(&pixels[..4], &thumbs[i][..4], "รอบแรกภาพที่ {i} ไม่ขึ้น");
            }
            read
        };

        // ---- device ตาย: ทิ้งทั้ง atlas ทั้ง device แล้วสร้างใหม่ทั้งชุด ----
        drop(device);
        drop(queue);
        let Some((device2, queue2, _)) = crate::device::headless_device() else {
            panic!("สร้าง device ใหม่ไม่ได้ — เส้นทางกู้ device จะพังในสถานการณ์จริง");
        };

        let (_textures2, mut atlas2) = fresh_atlas(&device2);
        let refilled = upload_all(&device2, &queue2, &mut atlas2, &thumbs);

        for (i, &slot) in refilled.iter().enumerate() {
            let pixels = read_slot(&device2, &queue2, &atlas2, slot);
            assert_eq!(
                pixels, before[i],
                "ภาพที่ {i} ไม่เหมือนเดิมหลังกู้ device — ผู้ใช้จะเห็นภาพหาย/สลับ"
            );
        }
        assert_eq!(
            atlas2.allocator().slots_in_use(),
            thumbs.len() as u32,
            "จำนวนช่องที่ใช้หลังเติมกลับต้องเท่าเดิมเป๊ะ"
        );
    }

    #[test]
    fn layers_needed_rounds_up() {
        assert_eq!(layers_needed(0), 1, "board ว่างก็ยังต้องมีอย่างน้อย 1 layer");
        assert_eq!(layers_needed(1), 1);
        assert_eq!(
            layers_needed(SLOTS_PER_LAYER as usize),
            1,
            "เต็มพอดี = 1 layer"
        );
        assert_eq!(
            layers_needed(SLOTS_PER_LAYER as usize + 1),
            2,
            "เกินมาหนึ่งภาพต้องได้ layer ที่สอง"
        );
        assert_eq!(layers_needed(1000), 4, "1000 ภาพ = 4 layer (docs/04 §4)");
    }

    /// ★ กับดักที่ทำให้ board ว่างเปล่าหลังกู้ device (เจอจริง 29 ก.ค. 2026)
    ///
    /// atlas ที่เพิ่งสร้างมี **0 layer** เพราะจองแบบ lazy → การเติมภาพกลับ
    /// **ต้องขยายให้พอก่อน** ไม่งั้นได้ `NeedsResize` ตั้งแต่ภาพแรก แล้วเส้นทาง
    /// เติมกลับจะเปลี่ยนทุกภาพเป็น placeholder ทั้ง board
    #[test]
    fn refilling_a_fresh_atlas_needs_a_resize_first() {
        let Some((device, queue, _)) = crate::device::gpu_for_test() else {
            return; // ข้าม/ล้ม ถูกตัดสินที่ gpu_for_test แล้ว (docs/08 §3.9 ข้อ 7)
        };
        let thumbs: Vec<Vec<u8>> = (0..5).map(|i| solid_thumb([i * 20, 30, 40, 255])).collect();
        let (_textures, mut atlas) = fresh_atlas(&device);

        // ---- negative control: ไม่ขยายก่อน = พังตั้งแต่ภาพแรก ----
        assert_eq!(atlas.layers_allocated(), 0, "atlas ใหม่ต้องยังไม่จอง layer");
        match atlas.upload(&queue, &thumbs[0]) {
            Err(AtlasError::NeedsResize { layers }) => assert_eq!(layers, 1),
            other => panic!("ต้องได้ NeedsResize แต่ได้ {other:?} — กับดักนี้หายไปแล้วหรือ?"),
        }

        // ---- ทำแบบที่ refill_atlas ทำจริง: ขยายให้พอก่อน แล้วเติมรวดเดียว ----
        atlas
            .resize(&device, layers_needed(thumbs.len()))
            .expect("ขยาย atlas ไม่ได้");
        for (i, pixels) in thumbs.iter().enumerate() {
            atlas
                .upload(&queue, pixels)
                .unwrap_or_else(|err| panic!("ภาพที่ {i} เติมกลับไม่ได้: {err}"));
        }
        assert_eq!(
            atlas.allocator().slots_in_use(),
            thumbs.len() as u32,
            "ต้องเติมกลับได้ครบทุกภาพ ไม่ใช่กลายเป็น placeholder"
        );
    }

    /// ★ negative control (docs/08 §3.9 ข้อ 1)
    ///
    /// ถ้า **ไม่** เติมภาพกลับ ช่องนั้นต้องอ่านได้เป็นพื้นที่ว่าง — พิสูจน์ว่า
    /// ตัวอ่าน pixel แยก "เติมแล้ว" กับ "ยังไม่เติม" ออกจากกันได้จริง
    /// ไม่งั้นเทสต์ข้างบนจะผ่านแม้การเติมกลับจะพังทั้งหมด
    #[test]
    fn a_fresh_atlas_reads_back_empty_until_it_is_refilled() {
        let Some((device, queue, _)) = crate::device::gpu_for_test() else {
            return; // ข้าม/ล้ม ถูกตัดสินที่ gpu_for_test แล้ว (docs/08 §3.9 ข้อ 7)
        };

        let filled = solid_thumb([200, 30, 40, 255]);
        let (_textures, mut atlas) = fresh_atlas(&device);
        // อัปโหลดช่องแรกช่องเดียว แล้วจองช่องที่สองทิ้งไว้โดยไม่เขียนอะไรลงไป
        let slots = upload_all(&device, &queue, &mut atlas, std::slice::from_ref(&filled));
        let untouched = atlas.allocator.allocate().expect("จองช่องที่สองไม่ได้");

        let written = read_slot(&device, &queue, &atlas, slots[0]);
        let blank = read_slot(&device, &queue, &atlas, untouched);

        assert_eq!(&written[..4], &filled[..4], "ช่องที่เขียนแล้วต้องมีสีจริง");
        assert!(
            blank.iter().all(|&byte| byte == 0),
            "ช่องที่ยังไม่เติมต้องว่าง — ถ้าไม่ว่าง แปลว่าตัวอ่านนี้เชื่อไม่ได้"
        );
    }
}
