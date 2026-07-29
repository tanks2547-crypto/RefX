//! Working texture cache — ชั้น B ของ docs/04 §4
//!
//! ภาพที่ซูมเข้าจนใหญ่กว่า thumbnail 128 px ได้ texture แยกของตัวเองที่ขนาด
//! power-of-two พอดีขนาดบนจอ พร้อม mip chain ทุกใบอยู่ใต้ **VRAM budget + LRU**
//!
//! ### ทำไมเป็น `texture_2d_array` ที่มี layer เดียว ไม่ใช่ `texture_2d`
//!
//! ★ เพื่อให้ใช้ **shader และ pipeline ตัวเดียวกับ atlas** ได้ทั้งหมด
//!
//! `quad.wgsl` ประกาศ `texture_2d_array<f32>` ถ้า working texture เป็น `texture_2d`
//! จะต้องมี shader คนละตัว + pipeline คนละตัว + bind group layout คนละตัว
//! ซึ่งแปลว่าโค้ด fragment เกือบทั้งหมด (grayscale, invert, กรอบเลือก, placeholder)
//! ถูกคัดลอกไว้สองที่แล้ว**เพี้ยนจากกันเมื่อมีคนแก้ข้างเดียว**
//! — ปัญหาเดียวกับที่ `read_file_guarded` ถูกทำให้ใช้เส้นทางเดียวเพื่อเลี่ยง
//!
//! ต้นทุนของ array ที่มี layer เดียวเทียบกับ texture ธรรมดา: ไม่มี
//!
//! spec: docs/04-rendering.md §4 ชั้น B, docs/05-memory-and-assets.md §1

use std::collections::HashMap;

use crate::texture::{TextureAllocator, TrackedTexture, VramError};

/// คีย์ของ working texture หนึ่งใบ
///
/// มี `size` อยู่ในคีย์ด้วยเพราะภาพเดียวกันมีได้หลายขนาดตามระดับซูม
/// การซูมเข้าจึงไม่ทิ้งใบเดิม แล้วซูมออกกลับมาก็ยังใช้ของเดิมได้ทันที
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct WorkingKey {
    /// คีย์ของเนื้อไฟล์
    pub hash: [u8; 32],
    /// ความกว้าง/สูงของ level 0
    pub size: u32,
}

/// texture หนึ่งใบพร้อม bind group ที่ผูกไว้แล้ว
struct Entry {
    /// ถือใบจอง VRAM ไว้ — drop แล้วโควตาคืนเอง (RAII)
    _texture: TrackedTexture,
    bind_group: wgpu::BindGroup,
    bytes: usize,
    /// ลำดับการใช้ล่าสุด — ตัวเลขมากคือเพิ่งใช้
    last_used: u64,
}

/// ลำดับที่ต้องไล่ออกเพื่อให้มีที่ว่างพอสำหรับ `wanted` ไบต์ — **ฟังก์ชันบริสุทธิ์**
///
/// ★ แยกออกมาด้วยเหตุผลเดียวกับที่ `SlotAllocator` ถูกแยกจาก `ThumbnailAtlas`:
/// การไล่ของออกตามลำดับการใช้งานเป็นตรรกะที่พลาดง่ายและมองไม่เห็นด้วยตา
/// ต้อง **ทดสอบได้โดยไม่ต้องมี GPU** ไม่งั้นจะรู้ว่ามันพังก็ต่อเมื่อ VRAM ผู้ใช้เต็มแล้ว
///
/// รับรายการ `(คีย์, ไบต์, ลำดับการใช้ล่าสุด)` คืนคีย์ที่ต้องทิ้งเรียงตามลำดับที่ทิ้ง
fn victims_for(
    entries: &[(WorkingKey, usize, u64)],
    used: usize,
    limit: usize,
    wanted: usize,
) -> Vec<WorkingKey> {
    let mut remaining: Vec<&(WorkingKey, usize, u64)> = entries.iter().collect();
    // เก่าสุดก่อน — `last_used` น้อย = ไม่ได้แตะมานานที่สุด
    remaining.sort_by_key(|(_, _, last_used)| *last_used);

    let mut victims = Vec::new();
    let mut used = used;
    for (key, bytes, _) in remaining {
        if used + wanted <= limit {
            break;
        }
        victims.push(*key);
        used = used.saturating_sub(*bytes);
    }
    victims
}

/// คลัง working texture ที่มีเพดานของตัวเอง
///
/// ★ ใช้ `bind_group_layout` **ตัวเดียวกับ atlas** จึงสลับ bind group ได้ในpipeline เดิม
pub struct WorkingCache {
    entries: HashMap<WorkingKey, Entry>,
    textures: TextureAllocator,
    sampler: wgpu::Sampler,
    /// เพดานของชั้นนี้ (ไบต์) — ครึ่งบนของงบ VRAM ทั้งหมด
    limit: usize,
    used: usize,
    /// นาฬิกาเชิงตรรกะสำหรับ LRU (เพิ่มทีละ 1 ทุกครั้งที่มีการใช้)
    clock: u64,
    /// จำนวนใบที่ถูกไล่ออกไปแล้ว — หลักฐานว่า LRU ทำงานจริง
    evicted: u64,
    /// รุ่นของ device ที่ texture ในคลังนี้ผูกอยู่
    ///
    /// ★ ทุกใบในนี้ตายพร้อม device — หลังกู้ device ต้อง **สร้างคลังใหม่ทั้งก้อน**
    /// (sampler กับ `TextureAllocator` ก็ผูกกับ device เดิมเหมือนกัน)
    /// ตัวเลขนี้มีไว้ให้ชั้นบนตรวจได้ว่าเผลอถือของรุ่นเก่าไว้หรือเปล่า
    generation: u64,
}

impl WorkingCache {
    /// format เดียวกับ atlas — sRGB ให้ GPU แปลง gamma ให้ฟรี (docs/04 §6)
    pub const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8UnormSrgb;

    /// สร้างคลังเปล่า **ยังไม่จอง VRAM เลย**
    ///
    /// `limit` คือครึ่งบนของงบ VRAM (อีกครึ่งเป็นของ atlas — docs/05 §1)
    #[must_use]
    pub fn new(
        device: &wgpu::Device,
        textures: TextureAllocator,
        limit: usize,
        generation: u64,
    ) -> Self {
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("refx-working-sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            // ★ ต้อง Linear ระหว่าง mip ไม่งั้นจะเห็นรอยต่อกระโดดตอนซูมออก
            mipmap_filter: wgpu::MipmapFilterMode::Linear,
            ..Default::default()
        });
        Self {
            entries: HashMap::new(),
            textures,
            sampler,
            limit: limit.max(1),
            used: 0,
            clock: 0,
            evicted: 0,
            generation,
        }
    }

    /// รุ่นของ device ที่คลังนี้ผูกอยู่ — ชั้นบนใช้ตรวจว่ายังตรงกับ device ปัจจุบันไหม
    #[must_use]
    pub fn generation(&self) -> u64 {
        self.generation
    }

    /// VRAM ที่คลังนี้ถืออยู่ (ไบต์)
    #[must_use]
    pub fn used(&self) -> usize {
        self.used
    }

    /// เพดานของคลังนี้ (ไบต์)
    #[must_use]
    pub fn limit(&self) -> usize {
        self.limit
    }

    /// จำนวน texture ที่ถืออยู่
    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// ไม่มี texture เลยหรือไม่
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// จำนวนใบที่ถูกไล่ออกไปแล้ว — ★ ตัวเลขนี้คือหลักฐานว่า LRU ทำงาน
    #[must_use]
    pub fn evicted(&self) -> u64 {
        self.evicted
    }

    /// ทิ้งทุกใบ
    ///
    /// ★ **ไม่พอสำหรับการกู้ device** — sampler กับ `TextureAllocator` ที่คลังนี้ถือ
    /// ก็ผูกกับ device เดิมด้วย เส้นทางกู้ device จึงสร้างคลังใหม่ทั้งก้อนแทน
    /// (ดู `DeviceBound` ใน `refx-ui::app`) ตัวนี้ไว้ใช้ตอนต้องคืน VRAM ด่วน
    pub fn clear(&mut self) {
        self.entries.clear();
        self.used = 0;
    }

    /// บอกว่าเฟรมนี้ใช้ใบนี้ — อัปเดตลำดับ LRU
    ///
    /// ★ แยกจาก [`WorkingCache::bind_group`] เพราะการวาดต้องถือ reference ของ
    /// หลายใบพร้อมกัน ถ้าเป็นเมธอดเดียวที่ยืมแบบ mutable จะถือได้ทีละใบเท่านั้น
    pub fn touch(&mut self, key: WorkingKey) {
        self.clock += 1;
        let clock = self.clock;
        if let Some(entry) = self.entries.get_mut(&key) {
            entry.last_used = clock;
        }
    }

    /// bind group ของ texture นี้ ถ้ามีอยู่ — **ไม่อัปเดต LRU** (ดู `touch`)
    #[must_use]
    pub fn bind_group(&self, key: WorkingKey) -> Option<&wgpu::BindGroup> {
        Some(&self.entries.get(&key)?.bind_group)
    }

    /// มีใบนี้อยู่ไหม — **ไม่นับเป็นการใช้งาน**
    #[must_use]
    pub fn contains(&self, key: WorkingKey) -> bool {
        self.entries.contains_key(&key)
    }

    /// อัปโหลด working texture ใหม่เข้าคลัง (ไล่ใบเก่าออกก่อนถ้าจำเป็น)
    ///
    /// `levels` คือ pixel ของแต่ละ mip level เรียงจากใหญ่ไปเล็ก
    ///
    /// # Errors
    /// คืน [`VramError`] เมื่อไล่ใบอื่นออกจนหมดแล้วยังไม่พอ — ผู้เรียกใช้ atlas ต่อไป
    /// (ภาพยังขึ้น แค่เบลอกว่า) ซึ่งดีกว่าไม่มีภาพ
    pub fn insert(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        bind_group_layout: &wgpu::BindGroupLayout,
        key: WorkingKey,
        levels: &[Vec<u8>],
    ) -> Result<(), VramError> {
        if levels.is_empty() || key.size == 0 {
            return Ok(()); // ไม่มีอะไรให้อัปโหลด — ไม่ใช่ error
        }
        if self.entries.contains_key(&key) {
            return Ok(()); // มีอยู่แล้ว
        }

        let mip_level_count = u32::try_from(levels.len()).unwrap_or(1);
        let descriptor = wgpu::TextureDescriptor {
            label: Some("refx-working"),
            size: wgpu::Extent3d {
                width: key.size,
                height: key.size,
                depth_or_array_layers: 1,
            },
            mip_level_count,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: Self::FORMAT,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        };
        let bytes = crate::texture::texture_bytes(&descriptor);

        // ★ ไล่ใบที่ไม่ได้ใช้นานที่สุดออกจนกว่าจะมีที่พอ — LRU (I-6)
        let snapshot: Vec<(WorkingKey, usize, u64)> = self
            .entries
            .iter()
            .map(|(key, entry)| (*key, entry.bytes, entry.last_used))
            .collect();
        for victim in victims_for(&snapshot, self.used, self.limit, bytes) {
            if let Some(entry) = self.entries.remove(&victim) {
                self.used = self.used.saturating_sub(entry.bytes);
                self.evicted += 1;
                tracing::debug!(
                    size = victim.size,
                    bytes = entry.bytes,
                    used_mb = self.used / (1 << 20),
                    "ไล่ working texture ออกตาม LRU"
                );
            }
        }

        let texture = self.textures.allocate(device, &descriptor)?;

        // อัปโหลดทีละ mip level
        let mut side = key.size;
        for (level, pixels) in levels.iter().enumerate() {
            let expected = side as usize * side as usize * 4;
            if pixels.len() < expected {
                tracing::error!(
                    level,
                    side,
                    got = pixels.len(),
                    expected,
                    "mip level ขนาดผิด"
                );
                break;
            }
            queue.write_texture(
                wgpu::TexelCopyTextureInfo {
                    texture: texture.texture(),
                    mip_level: u32::try_from(level).unwrap_or(0),
                    origin: wgpu::Origin3d::ZERO,
                    aspect: wgpu::TextureAspect::All,
                },
                &pixels[..expected],
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(side * 4),
                    rows_per_image: Some(side),
                },
                wgpu::Extent3d {
                    width: side,
                    height: side,
                    depth_or_array_layers: 1,
                },
            );
            side = (side / 2).max(1);
        }

        // ★ view เป็น D2Array ที่มี layer เดียว เพื่อให้เข้ากับ shader ของ atlas ได้
        let view = texture.create_view(&wgpu::TextureViewDescriptor {
            label: Some("refx-working-view"),
            dimension: Some(wgpu::TextureViewDimension::D2Array),
            ..Default::default()
        });
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("refx-working-bind"),
            layout: bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&self.sampler),
                },
            ],
        });

        self.clock += 1;
        self.entries.insert(
            key,
            Entry {
                _texture: texture,
                bind_group,
                bytes,
                last_used: self.clock,
            },
        );
        self.used += bytes;
        tracing::debug!(
            size = key.size,
            mips = mip_level_count,
            bytes,
            used_mb = self.used / (1 << 20),
            limit_mb = self.limit / (1 << 20),
            "เพิ่ม working texture"
        );
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    // เทสต์ต้อง panic! ได้เมื่อสร้าง device ใหม่ไม่สำเร็จ — นั่นคือความล้มเหลวจริง
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use super::*;

    fn key(size: u32, tag: u8) -> WorkingKey {
        WorkingKey {
            hash: [tag; 32],
            size,
        }
    }

    #[test]
    fn keys_separate_by_size_and_hash() {
        assert_ne!(key(512, 1), key(1024, 1), "ขนาดต่างกันคือคนละใบ");
        assert_ne!(key(512, 1), key(512, 2), "ภาพต่างกันคือคนละใบ");
        assert_eq!(key(512, 1), key(512, 1));
    }

    // ---------- ★ นโยบาย LRU (ไม่ต้องมี GPU) ----------

    fn entry(size: u32, tag: u8, bytes: usize, last_used: u64) -> (WorkingKey, usize, u64) {
        (key(size, tag), bytes, last_used)
    }

    #[test]
    fn nothing_is_evicted_while_there_is_room() {
        let entries = [entry(256, 1, 10, 1), entry(256, 2, 10, 2)];
        assert!(victims_for(&entries, 20, 100, 30).is_empty());
    }

    /// ★ ตัวที่ไม่ได้ใช้นานที่สุดต้องออกก่อนเสมอ
    #[test]
    fn least_recently_used_goes_first() {
        let entries = [
            entry(256, 1, 40, 5), // ใช้ล่าสุด
            entry(256, 2, 40, 1), // เก่าสุด → ต้องออกก่อน
            entry(256, 3, 40, 3),
        ];
        let victims = victims_for(&entries, 120, 100, 40);
        assert_eq!(victims.first(), Some(&key(256, 2)), "ต้องไล่ตัวเก่าสุดก่อน");
    }

    /// ไล่ออกพอดีเท่าที่จำเป็น ไม่ล้างทั้งคลัง
    #[test]
    fn evicts_only_as_much_as_needed() {
        let entries = [
            entry(256, 1, 30, 1),
            entry(256, 2, 30, 2),
            entry(256, 3, 30, 3),
        ];
        // ใช้อยู่ 90 เพดาน 100 ขอเพิ่ม 40 → ต้องว่างอย่างน้อย 30 = ไล่ออกใบเดียว
        let victims = victims_for(&entries, 90, 100, 40);
        assert_eq!(victims, vec![key(256, 1)]);
    }

    /// ★ ของชิ้นเดียวที่ใหญ่เกินเพดาน ต้องไม่ทำให้วนไม่จบ
    #[test]
    fn oversized_request_empties_the_cache_and_stops() {
        let entries = [entry(256, 1, 50, 1), entry(256, 2, 50, 2)];
        let victims = victims_for(&entries, 100, 100, 1000);
        assert_eq!(victims.len(), 2, "ไล่ออกหมดแล้วต้องหยุด ไม่วนต่อ");
    }

    /// ★ ซูมเข้า-ออกสลับไปมาต้องไม่ทำให้ VRAM ไต่ขึ้นเรื่อย ๆ
    ///
    /// จำลอง: สลับใช้สองขนาดไปมา 200 รอบ โดยเพดานรับได้แค่สองใบ
    /// ยอดที่ใช้ต้องไม่เกินเพดานสักครั้งเดียว
    #[test]
    fn alternating_zoom_never_grows_past_the_limit() {
        let limit = 100usize;
        let cost = 40usize;
        let mut live: Vec<(WorkingKey, usize, u64)> = Vec::new();
        let mut used = 0usize;
        let mut clock = 0u64;

        for round in 0..200u64 {
            let wanted = key(if round % 2 == 0 { 256 } else { 512 }, 1);
            clock += 1;
            if let Some(slot) = live.iter_mut().find(|(k, _, _)| *k == wanted) {
                slot.2 = clock; // ใช้ของเดิม
            } else {
                for victim in victims_for(&live, used, limit, cost) {
                    if let Some(pos) = live.iter().position(|(k, _, _)| *k == victim) {
                        used -= live.remove(pos).1;
                    }
                }
                live.push((wanted, cost, clock));
                used += cost;
            }
            assert!(used <= limit, "รอบที่ {round}: ใช้ {used} เกินเพดาน {limit}");
        }
        assert!(live.len() <= 2, "ค้างไว้ {} ใบ", live.len());
    }

    /// ขนาดที่ใช้คำนวณงบต้องรวม mip chain ด้วย ไม่งั้นเพดานจะถูกทะลุจริง ~33%
    #[test]
    fn budget_math_includes_the_mip_chain() {
        let flat = wgpu::TextureDescriptor {
            label: None,
            size: wgpu::Extent3d {
                width: 1024,
                height: 1024,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: WorkingCache::FORMAT,
            usage: wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        };
        let with_mips = wgpu::TextureDescriptor {
            mip_level_count: 11,
            ..flat.clone()
        };
        let a = crate::texture::texture_bytes(&flat);
        let b = crate::texture::texture_bytes(&with_mips);
        assert_eq!(a, 4 << 20, "1024² RGBA = 4 MB");
        assert_eq!(b, a + a / 3, "mip chain เพิ่มอีกราว 1/3");
    }

    // ---------- ★ GPU จริง: working texture หลัง device lost (P0-5 / P1-7) ----------

    /// layout ที่ใช้จริงตอนวาด — ขอจาก atlas เพราะ working texture ใช้ layout เดียวกัน
    fn layout(device: &wgpu::Device) -> wgpu::BindGroupLayout {
        let textures = TextureAllocator::with_limit(64 << 20);
        let atlas = crate::atlas::ThumbnailAtlas::new(device, &textures, 1).expect("atlas");
        // clone ไม่ได้ — สร้าง layout ชุดใหม่ที่หน้าตาเหมือนกันแทน
        drop(atlas);
        device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("refx-test-working-layout"),
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
        })
    }

    /// mip chain ปลอมขนาด `size` — ค่าไม่สำคัญ ที่สำคัญคือมันขึ้น GPU ได้จริง
    fn levels_for(size: u32) -> Vec<Vec<u8>> {
        let mut levels = Vec::new();
        let mut side = size;
        while side >= 1 {
            levels.push(vec![180u8; (side * side * 4) as usize]);
            if side == 1 {
                break;
            }
            side /= 2;
        }
        levels
    }

    /// ★ หลังกู้ device คลัง working texture ต้องเป็น **ของใหม่ที่ว่างเปล่า**
    /// แล้วเติมกลับได้จริงบน device ใบใหม่
    ///
    /// บั๊กที่เทสต์นี้กันไว้ (เจอจริง 29 ก.ค. 2026): `recover_device()` ไม่ได้สร้าง
    /// คลังใหม่ ทำให้หลังกู้ device แล้ว `contains()` ยังตอบ true สำหรับ texture
    /// ของ device ที่ตายไปแล้ว → เอา bind group นั้นไปวาด = ภาพหายทั้ง board
    /// และเพราะ `working_pending` ยังจำคีย์เดิมไว้ ภาพคมจึงไม่มีวันถูกขอใหม่เลย
    #[test]
    fn working_cache_is_rebuilt_empty_on_a_new_device() {
        let Some((device, queue, _)) = crate::device::gpu_for_test() else {
            return; // ข้าม/ล้ม ถูกตัดสินที่ gpu_for_test แล้ว (docs/08 §3.9 ข้อ 7)
        };

        let wanted = key(256, 7);
        let levels = levels_for(256);

        // ---- device รุ่น 0: มีภาพคมอยู่ในคลัง ----
        {
            let layout = layout(&device);
            let mut cache =
                WorkingCache::new(&device, TextureAllocator::with_limit(64 << 20), 32 << 20, 0);
            cache
                .insert(&device, &queue, &layout, wanted, &levels)
                .expect("อัปโหลด working texture ไม่ได้");
            assert!(cache.contains(wanted), "รอบแรกต้องมีของอยู่จริง");
            assert!(cache.used() > 0, "รอบแรกต้องกิน VRAM จริง");
            assert_eq!(cache.generation(), 0);
        }

        // ---- device ตาย แล้วกู้เป็นรุ่น 1 ----
        drop(device);
        drop(queue);
        let Some((device2, queue2, _)) = crate::device::headless_device() else {
            panic!("สร้าง device ใหม่ไม่ได้");
        };

        let layout2 = layout(&device2);
        let mut cache2 = WorkingCache::new(
            &device2,
            TextureAllocator::with_limit(64 << 20),
            32 << 20,
            1,
        );
        assert_eq!(cache2.generation(), 1, "คลังใหม่ต้องผูกกับ device รุ่นใหม่");
        assert!(
            !cache2.contains(wanted),
            "คลังหลังกู้ต้องว่าง — ถ้ายังตอบว่ามีของ เราจะวาดด้วย texture ของ device ที่ตายแล้ว"
        );
        assert_eq!(cache2.used(), 0, "โควตา VRAM ต้องเริ่มนับใหม่จากศูนย์");

        // ---- แล้วต้องเติมกลับได้จริงบน device ใบใหม่ ----
        cache2
            .insert(&device2, &queue2, &layout2, wanted, &levels)
            .expect("เติม working texture กลับบน device ใหม่ไม่ได้");
        assert!(cache2.contains(wanted), "ภาพคมต้องกลับมาหลังกู้ device");
        assert!(cache2.used() > 0);
    }
}
