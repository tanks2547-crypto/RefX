//! `TextureAllocator` — **ทางเดียวที่โปรเจกต์นี้สร้าง texture ได้**
//!
//! CLAUDE.md ห้าม `device.create_texture()` โดยไม่ผ่านที่นี่ เหตุผลคือ I-6:
//! ถ้าจองได้จากหลายที่ จะไม่มีใครรู้ว่า VRAM ถูกใช้ไปเท่าไหร่ แล้วเพดานก็ไร้ความหมาย
//!
//! ทุกใบที่จองไปคืนเองตอน drop (RAII) — texture ผูกกับ device ซึ่งหายได้ (P0-5)
//! ถ้าคืนด้วยมือจะรั่วทุกครั้งที่กู้ device แล้วเพดานจะเต็มถาวร
//!
//! spec: docs/05-memory-and-assets.md §2, docs/04-rendering.md §4

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use crate::device::GpuCapabilities;

/// จอง VRAM ไม่สำเร็จ
#[derive(Debug, thiserror::Error)]
pub enum VramError {
    /// เกินเพดาน VRAM ที่ตั้งไว้
    #[error(
        "หน่วยความจำการ์ดจอไม่พอ (ขอ {requested_mb} MB, ใช้อยู่ {used_mb} MB จากเพดาน {limit_mb} MB)\n\
         ลองปิด board ที่ไม่ได้ใช้ หรือเพิ่มเพดานหน่วยความจำในการตั้งค่า"
    )]
    OverBudget {
        /// ขนาดที่ขอ (MB)
        requested_mb: usize,
        /// ใช้ไปแล้ว (MB)
        used_mb: usize,
        /// เพดาน (MB)
        limit_mb: usize,
    },
}

/// เพดาน VRAM ที่คำนวณจากการ์ดจอจริง (docs/05 §2)
///
/// ★ iGPU แชร์ RAM ระบบ — ทุกไบต์ที่จองไปเบียด RAM ที่ Photoshop ใช้อยู่ตรง ๆ
/// จึงต้องให้น้อยกว่า dGPU มาก
#[must_use]
pub fn vram_limit_for(caps: &GpuCapabilities) -> usize {
    match caps.device_type {
        wgpu::DeviceType::IntegratedGpu | wgpu::DeviceType::Cpu => 128 << 20,
        // dGPU: 384 MB ตาม docs/05 §1
        _ => 384 << 20,
    }
}

/// ถังโควตา VRAM ที่ทุกคนใช้ร่วมกัน
#[derive(Debug)]
pub struct VramBudget {
    limit: usize,
    used: AtomicUsize,
}

impl VramBudget {
    /// สร้างถังขนาดที่กำหนด
    #[must_use]
    pub fn new(limit: usize) -> Self {
        Self {
            limit: limit.max(1),
            used: AtomicUsize::new(0),
        }
    }

    /// เพดาน (ไบต์)
    #[must_use]
    pub fn limit(&self) -> usize {
        self.limit
    }

    /// ใช้ไปแล้ว (ไบต์) — ต้องขึ้น status bar (I-6)
    #[must_use]
    pub fn used(&self) -> usize {
        self.used.load(Ordering::Relaxed)
    }

    /// เหลือเท่าไหร่
    #[must_use]
    pub fn available(&self) -> usize {
        self.limit.saturating_sub(self.used())
    }

    /// พยายามจอง — **ไม่รอ** ต่างจาก `RamBudget` เพราะการรอ VRAM บน UI thread
    /// จะทำให้จอค้าง ผู้เรียกต้อง evict LRU แล้วลองใหม่แทน
    fn try_reserve(&self, bytes: usize) -> Result<(), VramError> {
        // compare-exchange loop — จองจากหลายที่พร้อมกันได้โดยไม่เกินเพดาน
        let mut current = self.used.load(Ordering::Relaxed);
        loop {
            let next = current.saturating_add(bytes);
            if next > self.limit {
                return Err(VramError::OverBudget {
                    requested_mb: bytes / (1 << 20),
                    used_mb: current / (1 << 20),
                    limit_mb: self.limit / (1 << 20),
                });
            }
            match self.used.compare_exchange_weak(
                current,
                next,
                Ordering::AcqRel,
                Ordering::Relaxed,
            ) {
                Ok(_) => return Ok(()),
                Err(actual) => current = actual,
            }
        }
    }

    fn release(&self, bytes: usize) {
        self.used.fetch_sub(bytes, Ordering::AcqRel);
    }
}

/// texture ที่นับโควตาแล้ว — คืน VRAM ให้เองตอน drop
#[derive(Debug)]
pub struct TrackedTexture {
    texture: wgpu::Texture,
    budget: Arc<VramBudget>,
    bytes: usize,
}

impl TrackedTexture {
    /// texture ดิบ
    #[must_use]
    pub fn texture(&self) -> &wgpu::Texture {
        &self.texture
    }

    /// ขนาดที่จองไว้ (ไบต์)
    #[must_use]
    pub fn bytes(&self) -> usize {
        self.bytes
    }

    /// สร้าง view
    #[must_use]
    pub fn create_view(&self, desc: &wgpu::TextureViewDescriptor<'_>) -> wgpu::TextureView {
        self.texture.create_view(desc)
    }
}

impl Drop for TrackedTexture {
    fn drop(&mut self) {
        self.budget.release(self.bytes);
    }
}

/// ★ ทางเดียวที่สร้าง texture ได้ในโปรเจกต์นี้
///
/// ถือ `Arc<VramBudget>` ไว้ จึงตอบได้ตลอดว่า VRAM ถูกใช้ไปเท่าไหร่ (I-6)
#[derive(Debug, Clone)]
pub struct TextureAllocator {
    budget: Arc<VramBudget>,
}

impl TextureAllocator {
    /// สร้างตัวจัดสรรที่มีเพดานตามการ์ดจอจริง
    #[must_use]
    pub fn new(caps: &GpuCapabilities) -> Self {
        let limit = vram_limit_for(caps);
        tracing::info!(
            limit_mb = limit / (1 << 20),
            device_type = ?caps.device_type,
            "ตั้งเพดาน VRAM"
        );
        Self {
            budget: Arc::new(VramBudget::new(limit)),
        }
    }

    /// สร้างด้วยเพดานที่กำหนดเอง (เทสต์ / ตั้งค่าโดยผู้ใช้)
    #[must_use]
    pub fn with_limit(limit: usize) -> Self {
        Self {
            budget: Arc::new(VramBudget::new(limit)),
        }
    }

    /// ถังโควตา (อ่านตัวเลขไปแสดงบน status bar)
    #[must_use]
    pub fn budget(&self) -> &Arc<VramBudget> {
        &self.budget
    }

    /// จอง texture ตาม descriptor
    ///
    /// # Errors
    /// คืน [`VramError::OverBudget`] เมื่อจะทะลุเพดาน — ผู้เรียกต้อง evict แล้วลองใหม่
    pub fn allocate(
        &self,
        device: &wgpu::Device,
        desc: &wgpu::TextureDescriptor<'_>,
    ) -> Result<TrackedTexture, VramError> {
        let bytes = texture_bytes(desc);
        self.budget.try_reserve(bytes)?;

        // จองโควตาสำเร็จแล้วค่อยสร้างของจริง — ถ้าสลับลำดับจะนับไม่ตรงตอน error
        let texture = device.create_texture(desc);
        Ok(TrackedTexture {
            texture,
            budget: Arc::clone(&self.budget),
            bytes,
        })
    }
}

/// คำนวณขนาด VRAM ที่ texture หนึ่งใบใช้ (โดยประมาณ)
///
/// ประมาณจาก format ที่เราใช้จริงเท่านั้น — format อื่นถือว่า 4 ไบต์/pixel
/// (ประเมินสูงไว้ดีกว่าประเมินต่ำแล้วทะลุเพดานจริง)
#[must_use]
pub fn texture_bytes(desc: &wgpu::TextureDescriptor<'_>) -> usize {
    let bytes_per_pixel = match desc.format {
        wgpu::TextureFormat::R8Unorm => 1,
        wgpu::TextureFormat::Rg8Unorm => 2,
        _ => 4,
    };
    let per_layer = u64::from(desc.size.width)
        .saturating_mul(u64::from(desc.size.height))
        .saturating_mul(bytes_per_pixel);
    let mut total = per_layer.saturating_mul(u64::from(desc.size.depth_or_array_layers));

    // mip chain เพิ่มอีกประมาณ 1/3 ของ level 0
    if desc.mip_level_count > 1 {
        total = total.saturating_add(total / 3);
    }
    usize::try_from(total).unwrap_or(usize::MAX)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::*;

    fn caps(device_type: wgpu::DeviceType) -> GpuCapabilities {
        GpuCapabilities {
            adapter_name: "test".to_owned(),
            backend: wgpu::Backend::Noop,
            device_type,
            bc_compression: false,
            max_texture_dimension_2d: 8192,
        }
    }

    fn desc(w: u32, h: u32, layers: u32) -> wgpu::TextureDescriptor<'static> {
        wgpu::TextureDescriptor {
            label: None,
            size: wgpu::Extent3d {
                width: w,
                height: h,
                depth_or_array_layers: layers,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8UnormSrgb,
            usage: wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        }
    }

    /// ★ iGPU แชร์ RAM ระบบ — ต้องได้เพดานน้อยกว่า dGPU อย่างชัดเจน
    #[test]
    fn integrated_gpu_gets_smaller_budget() {
        let igpu = vram_limit_for(&caps(wgpu::DeviceType::IntegratedGpu));
        let dgpu = vram_limit_for(&caps(wgpu::DeviceType::DiscreteGpu));
        assert_eq!(igpu, 128 << 20, "iGPU ต้องได้ 128 MB ตาม docs/05 §2");
        assert_eq!(dgpu, 384 << 20, "dGPU ต้องได้ 384 MB ตาม docs/05 §1");
        assert!(igpu < dgpu);
    }

    #[test]
    fn atlas_layer_size_matches_spec() {
        // docs/04 §4: 2048×2048×4 layer RGBA8 = 64 MB
        assert_eq!(texture_bytes(&desc(2048, 2048, 4)), 64 << 20);
    }

    #[test]
    fn mip_chain_adds_a_third() {
        let mut d = desc(1024, 1024, 1);
        let flat = texture_bytes(&d);
        d.mip_level_count = 11;
        assert_eq!(texture_bytes(&d), flat + flat / 3);
    }

    #[test]
    fn over_budget_is_error_not_panic() {
        let budget = VramBudget::new(1 << 20);
        assert!(budget.try_reserve(512 * 1024).is_ok());
        let err = budget.try_reserve(1 << 20).unwrap_err();
        assert!(matches!(err, VramError::OverBudget { .. }), "ได้ {err:?}");
        // ของที่จองสำเร็จไปแล้วต้องไม่หาย
        assert_eq!(budget.used(), 512 * 1024);
    }

    #[test]
    fn release_returns_quota() {
        let budget = VramBudget::new(1000);
        budget.try_reserve(400).unwrap();
        assert_eq!(budget.used(), 400);
        assert_eq!(budget.available(), 600);
        budget.release(400);
        assert_eq!(budget.used(), 0);
    }

    /// จองจากหลายเธรดพร้อมกันต้องไม่ทะลุเพดาน
    #[test]
    fn concurrent_reserve_never_exceeds_limit() {
        let budget = Arc::new(VramBudget::new(1000));
        let mut handles = Vec::new();
        for _ in 0..8 {
            let budget = Arc::clone(&budget);
            handles.push(std::thread::spawn(move || {
                let mut held = 0usize;
                for _ in 0..200 {
                    if budget.try_reserve(100).is_ok() {
                        held += 100;
                    }
                    assert!(budget.used() <= 1000, "ทะลุเพดาน: {}", budget.used());
                }
                budget.release(held);
            }));
        }
        for h in handles {
            h.join().unwrap();
        }
        assert_eq!(budget.used(), 0, "คืนครบทุกใบ");
    }

    #[test]
    fn allocator_reports_limit_from_caps() {
        let alloc = TextureAllocator::new(&caps(wgpu::DeviceType::IntegratedGpu));
        assert_eq!(alloc.budget().limit(), 128 << 20);
        assert_eq!(alloc.budget().used(), 0);
    }

    #[test]
    fn tiny_limit_still_usable() {
        let budget = VramBudget::new(0); // ต้องถูกดันขึ้นเป็นอย่างน้อย 1
        assert!(budget.limit() >= 1);
    }
}
