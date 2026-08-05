//! `QuadInstance` (64 ไบต์) + instance buffer ที่จองครั้งเดียวใช้ตลอด
//!
//! ทุกภาพบน board = 1 instance ไม่ว่าจะเป็น thumbnail หรือภาพเต็ม
//! ที่ 1000 ภาพคิดเป็น 64 KB ต่อเฟรม — อัปโหลดทั้งก้อนได้สบาย
//!
//! spec: docs/04-rendering.md §3

use bytemuck::{Pod, Zeroable};

/// bit flag ของ `QuadInstance::flags` — ทำงานใน shader โดยไม่ต้องแตะ texture
pub mod flags {
    /// แปลงเป็นขาวดำด้วย luminance Rec. 709
    pub const GRAYSCALE: u32 = 1 << 0;
    /// กลับสี
    pub const INVERT: u32 = 1 << 1;
    /// วาดกรอบเลือก
    pub const SELECTED: u32 = 1 << 2;
    /// ยังไม่มี texture — วาดเป็นสี่เหลี่ยมทึบสี `tint` แทน
    ///
    /// docs/04 §8: ห้ามรอ ห้ามข้าม ผู้ใช้ต้องเห็น layout ทันที
    pub const PLACEHOLDER: u32 = 1 << 3;
}

/// ข้อมูลหนึ่งภาพที่ส่งให้ GPU
///
/// `transform` เป็น affine 2×3 `[a, b, c, d, tx, ty]` ไม่ใช่ mat4
/// เพราะงาน 2D ไม่ต้องใช้ 64 ไบต์ต่อ matrix — รวม pos/scale/rot/flip ไว้ครบแล้ว
#[repr(C)]
#[derive(Copy, Clone, Debug, PartialEq, Pod, Zeroable)]
pub struct QuadInstance {
    /// affine 2×3: `[a, b, c, d, tx, ty]` (unit quad → world)
    pub transform: [f32; 6],
    /// ตำแหน่งใน atlas หรือ crop rect: `[u0, v0, u1, v1]`
    pub uv_rect: [f32; 4],
    /// ★ คูณสี rgba — **`u8` ไม่ใช่ `f32`** (`VertexFormat::Unorm8x4`)
    ///
    /// GPU ขยาย `0..=255` เป็น `0.0..=1.0` ให้ฟรี shader จึงยังอ่านเป็น `vec4<f32>`
    /// เหมือนเดิมทุกประการ · ปลายทางเป็น framebuffer 8 บิตต่อช่องอยู่แล้ว
    /// `f32` ตรงนี้จึงให้ความละเอียดที่มองไม่เห็นความต่าง (docs/04 §3.5)
    ///
    /// **ข้อจำกัดที่ยอมรับ:** tint เกิน 1.0 ไม่ได้ (ไม่มี HDR multiply) —
    /// การทำให้สว่างกว่าต้นฉบับเป็นหน้าที่ของ [`Self::adjust`] ไม่ใช่ tint
    pub tint: [u8; 4],
    /// ชั้นใน texture array
    pub layer: u32,
    /// bitfield ดู [`flags`]
    pub flags: u32,
    /// ★ `[brightness, contrast]` เป็น **f32 เต็ม** ช่วง `-1.0..=1.0`
    ///
    /// ที่มาของ 8 ไบต์นี้คือที่ที่ทวงคืนจาก `tint` (docs/04 §3.5) —
    /// เดิมเคยยัดลง 16 บิตบนของ `flags` ซึ่งได้ความละเอียดแค่ 1/127 ต่อขั้น
    pub adjust: [f32; 2],
    /// เผื่อไว้ให้ครบ 64 ไบต์ — **ห้ามใช้โดยไม่แก้ `ATTRIBUTES` ให้ตรงกัน**
    pub reserved: u32,
}

// ★ ขนาดต้องเป็น 64 ไบต์เป๊ะ (docs/04 §3) ถ้าเปลี่ยนแล้วงบ VRAM/แบนด์วิดท์เปลี่ยนตาม
const _: () = assert!(size_of::<QuadInstance>() == 64);

impl QuadInstance {
    /// layout ของ instance buffer สำหรับ render pipeline
    ///
    /// WGSL ไม่มี `vec6` จึงต้องแยก `transform` เป็น `vec4 + vec2`
    /// ★ **ลำดับในนี้ต้องตรงกับลำดับฟิลด์ของ struct เป๊ะ** — มาโครคิด offset
    /// จากขนาดของ format ที่ไล่มาก่อนหน้า ไม่ได้อ่านจาก struct จริง
    ///
    /// location 6 เป็นของ vertex buffer มุม quad อยู่แล้ว `adjust` จึงไปที่ 7
    /// ส่วน `reserved` (4 ไบต์ท้าย) ไม่ถูก bind — `array_stride` ครอบมันไว้เฉย ๆ
    pub const ATTRIBUTES: [wgpu::VertexAttribute; 7] = wgpu::vertex_attr_array![
        0 => Float32x4,  // transform[0..4] = a, b, c, d   @ 0
        1 => Float32x2,  // transform[4..6] = tx, ty       @ 16
        2 => Float32x4,  // uv_rect                        @ 24
        3 => Unorm8x4,   // tint (u8 → 0..1 ให้ฟรี)        @ 40
        4 => Uint32,     // layer                          @ 44
        5 => Uint32,     // flags                          @ 48
        7 => Float32x2,  // adjust = [brightness, contrast] @ 52
    ];

    /// คำอธิบาย vertex buffer สำหรับ instance step mode
    #[must_use]
    pub const fn layout() -> wgpu::VertexBufferLayout<'static> {
        wgpu::VertexBufferLayout {
            array_stride: size_of::<Self>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Instance,
            attributes: &Self::ATTRIBUTES,
        }
    }

    /// สี่เหลี่ยมทึบสีเดียว ใช้ทดสอบและเป็น placeholder
    #[must_use]
    pub fn solid(x: f32, y: f32, width: f32, height: f32, rgba: [f32; 4]) -> Self {
        Self {
            transform: [width, 0.0, 0.0, height, x, y],
            uv_rect: [0.0, 0.0, 1.0, 1.0],
            tint: pack_tint(rgba),
            layer: 0,
            flags: flags::PLACEHOLDER,
            adjust: [0.0, 0.0],
            reserved: 0,
        }
    }

    /// ค่า `adjust` ที่แปลว่า "ไม่เปลี่ยนอะไร"
    ///
    /// ★ ตอนนี้เป็น **ศูนย์ตรง ๆ** ซึ่งเป็นค่า `Default` ด้วย — ต่างจากตอนที่ยัดลงบิต
    /// ที่ศูนย์แปลว่า "มืดสนิท" แล้วลืมใส่ทีเดียวภาพดำทั้งจอ (กับดักนั้นหายไปแล้ว)
    pub const NEUTRAL_ADJUST: [f32; 2] = [0.0, 0.0];
}

/// `0.0..=1.0` ต่อช่อง → ไบต์ที่ GPU จะขยายกลับเป็น `0.0..=1.0` ให้เอง
///
/// ค่าที่ไม่ใช่ตัวเลขตกเป็น 0 (I-4) — `NaN` ที่หลุดไปถึง GPU ทำให้ภาพหายทั้งจอ
/// โดยไม่มี error ที่ไหนเลย
#[must_use]
pub fn pack_tint(rgba: [f32; 4]) -> [u8; 4] {
    rgba.map(|channel| {
        if channel.is_finite() {
            #[expect(
                clippy::cast_possible_truncation,
                clippy::cast_sign_loss,
                reason = "clamp มาก่อนแล้ว ค่าอยู่ใน 0..=255 เสมอ"
            )]
            let byte = (channel.clamp(0.0, 1.0) * 255.0).round() as u8;
            byte
        } else {
            0
        }
    })
}

/// buffer ที่จองครั้งเดียวตอนเปิดโปรแกรมแล้วเขียนทับทุกเฟรม
///
/// ★ **ห้ามสร้าง buffer ใหม่ทุกเฟรม** — เป็นสาเหตุอันดับหนึ่งของ VRAM ที่โตเรื่อย ๆ
/// กับอาการกระตุก (docs/04 §3)
pub struct InstanceBuffer {
    buffer: wgpu::Buffer,
    capacity: u32,
}

impl InstanceBuffer {
    /// จำนวน instance ที่จองไว้ตั้งแต่แรก = 512 KB (docs/04 §3)
    pub const DEFAULT_CAPACITY: u32 = 8192;

    /// จอง buffer ขนาดคงที่
    pub fn new(device: &wgpu::Device, capacity: u32) -> Self {
        let capacity = capacity.max(1);
        let buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("refx-instances"),
            size: u64::from(capacity) * size_of::<QuadInstance>() as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        Self { buffer, capacity }
    }

    /// จำนวน instance สูงสุดที่ buffer นี้รับได้
    #[must_use]
    pub fn capacity(&self) -> u32 {
        self.capacity
    }

    /// buffer ดิบสำหรับผูกเข้า render pass
    #[must_use]
    pub fn buffer(&self) -> &wgpu::Buffer {
        &self.buffer
    }

    /// เขียน instance ลง buffer แล้วคืนจำนวนที่เขียนจริง
    ///
    /// ถ้าเกิน capacity จะเขียนเท่าที่ใส่ได้ — ผู้เรียกต้องแบ่งเป็นหลาย draw call
    /// (ดู [`InstanceBuffer::chunks`]) ไม่ใช่ปล่อยให้ภาพหายเงียบ ๆ
    pub fn write(&self, queue: &wgpu::Queue, instances: &[QuadInstance]) -> u32 {
        let n = instances.len().min(self.capacity as usize);
        if n == 0 {
            return 0;
        }
        queue.write_buffer(&self.buffer, 0, bytemuck::cast_slice(&instances[..n]));
        n as u32
    }

    /// แบ่ง instance เป็นก้อนที่พอดีกับ buffer
    ///
    /// หลัง culling แทบไม่มีทางเกิน 8192 แต่ต้องรองรับไว้ ไม่งั้นภาพหายโดยไม่มีใครรู้
    pub fn chunks<'a>(
        &self,
        instances: &'a [QuadInstance],
    ) -> impl Iterator<Item = &'a [QuadInstance]> {
        instances.chunks(self.capacity as usize)
    }
}

#[cfg(test)]
mod tests {
    // เทียบ float ตรง ๆ ได้ในเทสต์: ค่าที่ assert คือค่าคงที่หลัง clamp/ประกอบ struct
    // ซึ่งต้องเท่ากันเป๊ะ ไม่ใช่ผลจากการคำนวณทศนิยมสะสม
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::float_cmp)]

    use super::*;

    /// ★★ offset ที่มาโครคำนวณ ต้องตรงกับ offset จริงของฟิลด์ **ทุกตัว**
    ///
    /// `vertex_attr_array!` ไล่บวก offset จากขนาดของ format ที่ประกาศมาก่อนหน้า
    /// **มันไม่ได้อ่านจาก struct จริง** — สลับลำดับฟิลด์หรือเปลี่ยนชนิดเมื่อไหร่
    /// ทั้งสองฝั่งจะเงียบ ๆ ไม่ตรงกัน แล้ว GPU จะอ่าน `tint` เป็น `layer`
    /// ซึ่งเป็นความผิดพลาดที่ **ไม่มี error ที่ไหนเลย** มีแค่ภาพที่ดูแปลก ๆ
    #[test]
    fn every_attribute_points_at_the_field_it_claims_to() {
        let base = std::ptr::from_ref::<QuadInstance>(&ZERO).cast::<u8>();
        let offset_of = |field: *const u8| {
            u64::try_from(field as usize - base as usize).expect("offset ต้องเป็นบวก")
        };
        let want = [
            offset_of(std::ptr::from_ref(&ZERO.transform).cast()),
            offset_of(std::ptr::from_ref(&ZERO.transform[4]).cast()),
            offset_of(std::ptr::from_ref(&ZERO.uv_rect).cast()),
            offset_of(std::ptr::from_ref(&ZERO.tint).cast()),
            offset_of(std::ptr::from_ref(&ZERO.layer).cast()),
            offset_of(std::ptr::from_ref(&ZERO.flags).cast()),
            offset_of(std::ptr::from_ref(&ZERO.adjust).cast()),
        ];
        let got: Vec<u64> = QuadInstance::ATTRIBUTES
            .iter()
            .map(|attr| attr.offset)
            .collect();
        assert_eq!(got, want, "offset ของ vertex attribute ไม่ตรงกับฟิลด์จริง");

        // และช่องสุดท้ายต้องยังอยู่ในขอบเขต 64 ไบต์
        let last = QuadInstance::ATTRIBUTES.last().expect("ต้องมีอย่างน้อยหนึ่ง");
        assert!(last.offset + last.format.size() <= 64);
    }

    /// ค่าอ้างอิงสำหรับคำนวณ offset — ต้องเป็น `static` เพื่อให้ที่อยู่นิ่ง
    static ZERO: QuadInstance = QuadInstance {
        transform: [0.0; 6],
        uv_rect: [0.0; 4],
        tint: [0; 4],
        layer: 0,
        flags: 0,
        adjust: [0.0; 2],
        reserved: 0,
    };

    /// `tint` ที่เป็นไบต์ต้องยังกลับมาเป็นค่าเดิมในระดับที่ตาแยกไม่ออก
    #[test]
    fn packing_a_tint_round_trips_within_one_step() {
        for value in [0.0_f32, 0.25, 0.5, 0.75, 1.0, 0.333] {
            let packed = pack_tint([value; 4])[0];
            let back = f32::from(packed) / 255.0;
            assert!(
                (back - value).abs() <= 1.0 / 255.0,
                "{value} → {packed} → {back}"
            );
        }
        // I-4: ค่าที่พังต้องไม่กลายเป็นขยะ
        assert_eq!(
            pack_tint([f32::NAN, f32::INFINITY, -5.0, 2.0]),
            [0, 0, 0, 255]
        );
    }

    #[test]
    fn instance_is_exactly_64_bytes() {
        // ตัวเลขนี้อยู่ใน docs/04 §3 และใช้คำนวณงบ VRAM — เปลี่ยนแล้วต้องแก้ spec ด้วย
        assert_eq!(size_of::<QuadInstance>(), 64);
    }

    #[test]
    fn thousand_instances_fit_in_64kb() {
        // ข้อกำหนดใน docs/04 §3: 1000 ภาพ = 64 KB ต่อเฟรม
        assert_eq!(size_of::<QuadInstance>() * 1000, 64_000);
    }

    #[test]
    fn default_capacity_is_512kb() {
        let bytes = InstanceBuffer::DEFAULT_CAPACITY as usize * size_of::<QuadInstance>();
        assert_eq!(bytes, 512 * 1024);
    }

    #[test]
    fn solid_sets_placeholder_flag() {
        let quad = QuadInstance::solid(1.0, 2.0, 3.0, 4.0, [1.0, 0.0, 0.0, 1.0]);
        assert_ne!(quad.flags & flags::PLACEHOLDER, 0);
        assert_eq!(quad.transform, [3.0, 0.0, 0.0, 4.0, 1.0, 2.0]);
    }

    #[test]
    fn flags_do_not_overlap() {
        let all = [
            flags::GRAYSCALE,
            flags::INVERT,
            flags::SELECTED,
            flags::PLACEHOLDER,
        ];
        for (i, a) in all.iter().enumerate() {
            for b in &all[i + 1..] {
                assert_eq!(a & b, 0, "flag ทับกัน: {a:#b} กับ {b:#b}");
            }
        }
    }

    #[test]
    fn chunks_split_oversized_input() {
        // ไม่ต้องมี GPU — ทดสอบเฉพาะ logic การแบ่ง
        let instances = vec![QuadInstance::solid(0.0, 0.0, 1.0, 1.0, [0.0; 4]); 10_000];
        let capacity = 8192usize;
        let chunks: Vec<_> = instances.chunks(capacity).collect();
        assert_eq!(chunks.len(), 2);
        assert_eq!(chunks[0].len(), 8192);
        assert_eq!(chunks[1].len(), 1808);
        // ห้ามมีภาพหาย
        assert_eq!(
            chunks.iter().map(|c| c.len()).sum::<usize>(),
            instances.len()
        );
    }
}
