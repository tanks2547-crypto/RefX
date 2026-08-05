//! render pipeline ของ instanced quad + กล้อง 2 มิติ
//!
//! ทั้ง board วาดด้วย draw call เดียว (หรือไม่กี่ครั้งถ้าเกิน 8192 instance)
//! ไม่มี depth buffer — เรียงด้วย painter's algorithm ตาม `z_order` ที่ sort มาแล้ว
//!
//! spec: docs/04-rendering.md §2, §3, §9

use bytemuck::{Pod, Zeroable};
use wgpu::util::DeviceExt as _;

use crate::instance::{InstanceBuffer, QuadInstance};

/// affine world→clip ที่ส่งให้ shader
///
/// เก็บเป็นสอง `vec4` เพราะ uniform buffer ต้อง align 16 ไบต์
#[repr(C)]
#[derive(Copy, Clone, Debug, Pod, Zeroable)]
pub struct CameraUniform {
    /// a, b, c, d ของ affine 2×3
    pub view_a: [f32; 4],
    /// tx, ty, **grayscale ทั้ง board** (0/1), แล้วเว้นอีก 1 ช่องให้ครบ 16 ไบต์
    pub view_b: [f32; 4],
}

impl CameraUniform {
    /// กล้องที่แปลง world (พิกเซล, origin ซ้ายบน, y ชี้ลง) → clip space
    ///
    /// clip space ของ wgpu คือ x,y ∈ [-1, 1] และ **y ชี้ขึ้น** จึงต้องกลับแกน y
    #[must_use]
    pub fn from_viewport(width: u32, height: u32) -> Self {
        // กันหารศูนย์ตอนหน้าต่างถูกย่อ
        let w = width.max(1) as f32;
        let h = height.max(1) as f32;
        Self {
            view_a: [2.0 / w, 0.0, 0.0, -2.0 / h],
            view_b: [-1.0, 1.0, 0.0, 0.0],
        }
    }

    /// เปิด/ปิด grayscale ระดับ board (docs/03 §2 — "uniform ตัวเดียว")
    ///
    /// ★ สลับสวิตช์นี้ที่ 1000 ภาพเขียน uniform 32 ไบต์ครั้งเดียว **ไม่แตะ
    /// instance buffer และไม่อัป texture เลย** ซึ่งคือทั้งหมดที่ทำให้ปุ่ม `G`
    /// ราคาเกือบศูนย์ตามที่ ROADMAP P2-8 กำหนด
    #[must_use]
    pub fn with_grayscale(mut self, on: bool) -> Self {
        self.view_b[2] = if on { 1.0 } else { 0.0 };
        self
    }

    /// สร้างจาก affine 2×3 `[a, b, c, d, tx, ty]`
    ///
    /// คณิตศาสตร์ของกล้องอยู่ที่ `refx_core::view::Camera::to_clip_affine` ที่เดียว
    /// — ที่นี่แค่จัด layout ให้ตรงกับ uniform buffer ห้ามคำนวณซ้ำ ไม่งั้นสองที่จะเพี้ยนคนละทาง
    #[must_use]
    pub fn from_affine(affine: [f32; 6]) -> Self {
        // ค่าที่ไม่ finite ห้ามหลุดไปถึง GPU (I-4) — ตกกลับไปใช้กล้องเอกลักษณ์
        if !affine.iter().all(|v| v.is_finite()) {
            return Self {
                view_a: [1.0, 0.0, 0.0, 1.0],
                view_b: [0.0, 0.0, 0.0, 0.0],
            };
        }
        Self {
            view_a: [affine[0], affine[1], affine[2], affine[3]],
            view_b: [affine[4], affine[5], 0.0, 0.0],
        }
    }
}

/// มุมของ unit quad — vertex buffer ก้อนเดียวใช้ร่วมกันทุก instance
const QUAD_CORNERS: [[f32; 2]; 4] = [[0.0, 0.0], [1.0, 0.0], [0.0, 1.0], [1.0, 1.0]];

/// instance ชุดหนึ่งที่ใช้ texture เดียวกัน — หน่วยของ [`QuadPipeline::draw_batches`]
#[derive(Clone, Copy)]
pub struct DrawBatch<'a> {
    /// texture ที่ instance ชุดนี้ใช้ (atlas หรือ working texture ใบใดใบหนึ่ง)
    pub bind_group: &'a wgpu::BindGroup,
    /// instance ที่ใช้ texture นั้น
    pub instances: &'a [QuadInstance],
}

/// pipeline + resource ที่ผูกกับ device หนึ่งตัว
///
/// ★ ผูกกับ device — หลัง device lost ต้องสร้างใหม่ทั้งก้อน (docs/04 §7 ข้อ 3)
pub struct QuadPipeline {
    pipeline: wgpu::RenderPipeline,
    camera_buffer: wgpu::Buffer,
    camera_bind_group: wgpu::BindGroup,
    vertex_buffer: wgpu::Buffer,
    instances: InstanceBuffer,
}

impl QuadPipeline {
    /// สร้าง pipeline ทั้งชุด
    ///
    /// `format` ต้องเป็น format ของ surface (sRGB) เพื่อให้ GPU แปลง gamma ให้ฟรี
    pub fn new(
        device: &wgpu::Device,
        format: wgpu::TextureFormat,
        atlas_layout: &wgpu::BindGroupLayout,
    ) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("refx-quad-shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("quad.wgsl").into()),
        });

        let camera_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("refx-camera"),
            contents: bytemuck::bytes_of(&CameraUniform::from_viewport(1, 1)),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });

        let camera_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("refx-camera-layout"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                // ★ ต้องมองเห็นทั้งสอง stage — vertex ใช้ affine ส่วน fragment อ่าน
                //   สวิตช์ grayscale ระดับ board (view_b.z) ตั้งแต่ P2-8
                //   ถ้าปล่อยเป็น VERTEX อย่างเดียว wgpu จะปฏิเสธ pipeline ตอนสร้าง
                visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });

        let camera_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("refx-camera-bind"),
            layout: &camera_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: camera_buffer.as_entire_binding(),
            }],
        });

        let vertex_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("refx-quad-corners"),
            contents: bytemuck::cast_slice(&QUAD_CORNERS),
            usage: wgpu::BufferUsages::VERTEX,
        });

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("refx-quad-layout"),
            // group 0 = กล้อง, group 1 = atlas
            bind_group_layouts: &[Some(&camera_layout), Some(atlas_layout)],
            // wgpu 29 เปลี่ยนจาก push_constant_ranges เป็น immediate_size
            immediate_size: 0,
        });

        // มุมของ unit quad อยู่ที่ location 6 (0..5 เป็นของ instance)
        let corner_attrs = wgpu::vertex_attr_array![6 => Float32x2];
        let corner_layout = wgpu::VertexBufferLayout {
            array_stride: size_of::<[f32; 2]>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &corner_attrs,
        };

        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("refx-quad-pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                buffers: &[corner_layout, QuadInstance::layout()],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    // ภาพมี opacity ได้ → ต้อง alpha blend
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleStrip,
                strip_index_format: None,
                front_face: wgpu::FrontFace::Ccw,
                // ภาพ flip ได้ → determinant ของ affine ติดลบได้ ห้าม cull
                cull_mode: None,
                unclipped_depth: false,
                polygon_mode: wgpu::PolygonMode::Fill,
                conservative: false,
            },
            // docs/04 §2: ไม่มี depth buffer ไม่มี MSAA
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview_mask: None, // ไม่ใช้ multiview (VR)
            cache: None,
        });

        Self {
            pipeline,
            camera_buffer,
            camera_bind_group,
            vertex_buffer,
            instances: InstanceBuffer::new(device, InstanceBuffer::DEFAULT_CAPACITY),
        }
    }

    /// อัปเดตกล้อง — เขียนทับ buffer เดิม ไม่สร้างใหม่
    pub fn set_camera(&self, queue: &wgpu::Queue, camera: CameraUniform) {
        queue.write_buffer(&self.camera_buffer, 0, bytemuck::bytes_of(&camera));
    }

    /// จำนวน instance ที่ buffer รับได้ต่อหนึ่ง draw call
    #[must_use]
    pub fn capacity(&self) -> u32 {
        self.instances.capacity()
    }

    /// หนึ่งก้อนของการวาด — instance ชุดหนึ่งที่ใช้ texture เดียวกัน
    ///
    /// ใช้กับ working texture (docs/04 §4 ชั้น B) ซึ่งมี bind group ต่อภาพ
    /// ปกติมีในจอพร้อมกัน < 30 ตัว → < 30 draw call ซึ่งยอมรับได้ตาม spec
    pub fn draw_batches(
        &self,
        queue: &wgpu::Queue,
        pass: &mut wgpu::RenderPass<'_>,
        batches: &[DrawBatch<'_>],
    ) -> u32 {
        let total: usize = batches.iter().map(|b| b.instances.len()).sum();
        if total == 0 {
            return 0;
        }

        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &self.camera_bind_group, &[]);
        pass.set_vertex_buffer(0, self.vertex_buffer.slice(..));
        pass.set_vertex_buffer(1, self.instances.buffer().slice(..));

        // ★ ทุกก้อนเขียนลง buffer เดียวกันที่ **offset ต่างกัน** แล้ววาดด้วยช่วง
        //   instance ของตัวเอง ห้ามเขียนทับที่ offset 0 ทุกก้อน เพราะ GPU ทำงาน
        //   ตอน submit ไม่ใช่ตอนเรียก — ก้อนก่อนหน้าจะกลายเป็นข้อมูลของก้อนสุดท้ายหมด
        let stride = size_of::<QuadInstance>() as u64;
        let mut first: u32 = 0;
        let mut draw_calls = 0;
        for batch in batches {
            if batch.instances.is_empty() {
                continue;
            }
            let room = self.instances.capacity().saturating_sub(first) as usize;
            let n = batch.instances.len().min(room);
            if n == 0 {
                tracing::warn!(
                    capacity = self.instances.capacity(),
                    "instance buffer is full — the remaining images are not drawn this frame"
                );
                break;
            }
            queue.write_buffer(
                self.instances.buffer(),
                u64::from(first) * stride,
                bytemuck::cast_slice(&batch.instances[..n]),
            );
            pass.set_bind_group(1, batch.bind_group, &[]);
            let n = u32::try_from(n).unwrap_or(0);
            pass.draw(0..4, first..first + n);
            first += n;
            draw_calls += 1;
        }
        draw_calls
    }

    /// วาดทุก instance ที่ให้มา
    ///
    /// ถ้าเกิน capacity จะแบ่งเป็นหลาย draw call ให้เอง — ภาพไม่หาย
    /// คืนจำนวน draw call ที่ใช้จริง (ใช้ตรวจใน benchmark)
    pub fn draw(
        &self,
        queue: &wgpu::Queue,
        pass: &mut wgpu::RenderPass<'_>,
        atlas_bind_group: &wgpu::BindGroup,
        instances: &[QuadInstance],
    ) -> u32 {
        if instances.is_empty() {
            return 0;
        }

        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &self.camera_bind_group, &[]);
        pass.set_bind_group(1, atlas_bind_group, &[]);
        pass.set_vertex_buffer(0, self.vertex_buffer.slice(..));

        let mut draw_calls = 0;
        for chunk in self.instances.chunks(instances) {
            let n = self.instances.write(queue, chunk);
            if n == 0 {
                continue;
            }
            pass.set_vertex_buffer(1, self.instances.buffer().slice(..));
            // 4 vertex ของ unit quad × n instance
            pass.draw(0..4, 0..n);
            draw_calls += 1;
        }
        draw_calls
    }
}

#[cfg(test)]
mod tests {
    // เทียบ float ตรง ๆ ได้ในเทสต์: ค่าที่ assert คือค่าคงที่หลัง clamp/ประกอบ struct
    // ซึ่งต้องเท่ากันเป๊ะ ไม่ใช่ผลจากการคำนวณทศนิยมสะสม
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::float_cmp)]

    use super::*;

    /// แปลงจุดด้วย affine ของกล้อง (เลียนแบบ apply_affine ใน WGSL)
    fn project(cam: &CameraUniform, p: [f32; 2]) -> [f32; 2] {
        [
            cam.view_a[0] * p[0] + cam.view_a[2] * p[1] + cam.view_b[0],
            cam.view_a[1] * p[0] + cam.view_a[3] * p[1] + cam.view_b[1],
        ]
    }

    fn close(a: f32, b: f32) -> bool {
        (a - b).abs() < 1e-5
    }

    #[test]
    fn viewport_maps_corners_to_clip_space() {
        let cam = CameraUniform::from_viewport(800, 600);
        // ซ้ายบนของหน้าจอ → (-1, 1) ใน clip space (y ชี้ขึ้น)
        let tl = project(&cam, [0.0, 0.0]);
        assert!(close(tl[0], -1.0) && close(tl[1], 1.0), "ซ้ายบนผิด: {tl:?}");
        // ขวาล่าง → (1, -1)
        let br = project(&cam, [800.0, 600.0]);
        assert!(close(br[0], 1.0) && close(br[1], -1.0), "ขวาล่างผิด: {br:?}");
    }

    #[test]
    fn from_affine_preserves_layout() {
        let cam = CameraUniform::from_affine([1.0, 2.0, 3.0, 4.0, 5.0, 6.0]);
        assert_eq!(cam.view_a, [1.0, 2.0, 3.0, 4.0]);
        assert_eq!(cam.view_b, [5.0, 6.0, 0.0, 0.0]);
    }

    /// ค่าจากไฟล์อาจเป็น NaN/inf ได้ (I-4) ห้ามส่งต่อไปให้ GPU
    #[test]
    fn from_affine_rejects_non_finite_values() {
        for bad in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
            let cam = CameraUniform::from_affine([bad, 0.0, 0.0, 1.0, 0.0, 0.0]);
            assert!(
                cam.view_a.iter().all(|v| v.is_finite())
                    && cam.view_b.iter().all(|v| v.is_finite()),
                "ค่า {bad} หลุดไปถึง GPU"
            );
        }
    }

    #[test]
    fn zero_viewport_does_not_divide_by_zero() {
        let cam = CameraUniform::from_viewport(0, 0);
        assert!(cam.view_a.iter().all(|v| v.is_finite()));
    }

    #[test]
    fn camera_uniform_is_32_bytes() {
        // ต้อง align 16 ไบต์สำหรับ uniform buffer
        assert_eq!(size_of::<CameraUniform>(), 32);
        assert_eq!(size_of::<CameraUniform>() % 16, 0);
    }

    // ---------- ★ P2-8: พิสูจน์ว่า **pixel เปลี่ยนจริง** ไม่ใช่แค่ธงถูกตั้ง ----------

    /// วาด quad หนึ่งใบเต็มเป้าแล้วอ่านสีกลับมา — เป้าเป็น `Rgba8Unorm` (ไม่ใช่ sRGB)
    /// เพื่อให้เลขที่อ่านได้เป็นค่าเดียวกับที่ shader คำนวณ ไม่ต้องถอด gamma ก่อนเทียบ
    ///
    /// ใช้ธง `PLACEHOLDER` เพื่อให้ shader ใช้ `tint` เป็นสีต้นทางตรง ๆ —
    /// เทสต์นี้สนใจ **ขั้นตอนหลังการ sample** (brightness/contrast/grayscale/invert)
    /// ไม่ใช่การอ่าน texture ซึ่งมีเทสต์ของตัวเองอยู่แล้วใน `atlas.rs`
    fn render_pixel(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        atlas: &crate::atlas::ThumbnailAtlas,
        pipeline: &QuadPipeline,
        instance: QuadInstance,
        camera: CameraUniform,
    ) -> [u8; 4] {
        const SIDE: u32 = 1;
        // wgpu บังคับให้แถวของ buffer ปลายทางหาร 256 ลงตัว
        const ROW: u32 = 256;

        let target = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("refx-test-target"),
            size: wgpu::Extent3d {
                width: SIDE,
                height: SIDE,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let view = target.create_view(&wgpu::TextureViewDescriptor::default());

        let readback = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("refx-test-pixel"),
            size: u64::from(ROW),
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });

        pipeline.set_camera(queue, camera);

        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("refx-test-draw"),
        });
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("refx-test-pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            let batch = DrawBatch {
                bind_group: atlas.bind_group(),
                instances: std::slice::from_ref(&instance),
            };
            pipeline.draw_batches(queue, &mut pass, &[batch]);
        }
        encoder.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture: &target,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyBufferInfo {
                buffer: &readback,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(ROW),
                    rows_per_image: Some(SIDE),
                },
            },
            wgpu::Extent3d {
                width: SIDE,
                height: SIDE,
                depth_or_array_layers: 1,
            },
        );
        queue.submit(std::iter::once(encoder.finish()));

        readback.slice(..).map_async(wgpu::MapMode::Read, |_| {});
        // ★ ต้องมี timeout เสมอ — เทสต์ที่ค้างตลอดกาลใน CI แย่กว่าเทสต์ที่ล้ม
        device
            .poll(wgpu::PollType::Wait {
                submission_index: None,
                timeout: Some(std::time::Duration::from_secs(10)),
            })
            .expect("รอ GPU ไม่สำเร็จ");
        let bytes = readback.slice(..).get_mapped_range().to_vec();
        readback.unmap();
        [bytes[0], bytes[1], bytes[2], bytes[3]]
    }

    /// เตรียม device + atlas (1 layer) + pipeline สำหรับเทสต์กลุ่มนี้
    fn pixel_harness() -> Option<(
        wgpu::Device,
        wgpu::Queue,
        crate::atlas::ThumbnailAtlas,
        QuadPipeline,
    )> {
        let (device, queue, caps) = crate::device::gpu_for_test()?;
        let allocator = crate::texture::TextureAllocator::new(&caps);
        let mut atlas = crate::atlas::ThumbnailAtlas::new(&device, &allocator, 2).ok()?;
        // atlas ที่เพิ่งสร้างมี 0 layer (จองแบบ lazy) — ต้องมีอย่างน้อยหนึ่งชั้น
        // ไม่งั้น texture view ที่ผูกเข้า bind group ไม่มีอะไรให้ sample
        atlas.resize(&device, 1).ok()?;
        let pipeline = QuadPipeline::new(
            &device,
            wgpu::TextureFormat::Rgba8Unorm,
            atlas.bind_group_layout(),
        );
        Some((device, queue, atlas, pipeline))
    }

    /// quad ที่กินเป้า 1×1 พอดี พร้อมสีต้นทางที่กำหนดเอง
    fn flat_quad(rgba: [f32; 4], extra_flags: u32) -> QuadInstance {
        QuadInstance {
            transform: [1.0, 0.0, 0.0, 1.0, 0.0, 0.0],
            uv_rect: [0.0, 0.0, 1.0, 1.0],
            tint: crate::instance::pack_tint(rgba),
            layer: 0,
            flags: crate::instance::flags::PLACEHOLDER | extra_flags,
            adjust: QuadInstance::NEUTRAL_ADJUST,
            reserved: 0,
        }
    }

    /// กล้องที่แมป world 0..1 ให้เต็มเป้า 1×1
    fn full_target_camera() -> CameraUniform {
        CameraUniform::from_viewport(1, 1)
    }

    /// ★ ค่ากลางต้องไม่เปลี่ยนสีเลย — ถ้าข้อนี้พัง แปลว่าบิต adjust ถูกอ่านผิด
    /// แล้วภาพ**ทุกใบ**บนจอจะเพี้ยนพร้อมกัน (บิตศูนย์ = มืดสนิท ไม่ใช่ "ไม่เปลี่ยน")
    #[test]
    fn neutral_adjustment_leaves_every_pixel_untouched() {
        let Some((device, queue, atlas, pipeline)) = pixel_harness() else {
            eprintln!("ข้าม: ไม่มี GPU adapter");
            return;
        };
        let source = [0.4, 0.6, 0.8, 1.0];
        let got = render_pixel(
            &device,
            &queue,
            &atlas,
            &pipeline,
            flat_quad(source, 0),
            full_target_camera(),
        );
        for (channel, want) in got[..3].iter().zip(source) {
            let want = (want * 255.0_f32).round() as u8;
            assert!(
                channel.abs_diff(want) <= 1,
                "ค่ากลางต้องคืนสีเดิม: ได้ {got:?} ควรได้ราว ๆ {want}"
            );
        }
    }

    /// ★★ brightness/contrast **ต้องเปลี่ยน pixel จริง** ไม่ใช่แค่ธงถูกส่งไป
    ///
    /// ฝั่ง shader ของสองค่านี้เขียนใหม่ทั้งหมดตอน P2-8 (ต่างจาก grayscale/invert
    /// ที่ shader รออยู่แล้ว) — ถ้าเขียนแต่ฝั่ง Rust ประตู audit จะยังเขียว
    /// เพราะธงเปลี่ยนจริง **แต่ภาพบนจอไม่ขยับเลย** เทสต์นี้คือด่านที่จับข้อนั้น
    #[test]
    fn brightness_and_contrast_actually_change_the_rendered_pixel() {
        let Some((device, queue, atlas, pipeline)) = pixel_harness() else {
            eprintln!("ข้าม: ไม่มี GPU adapter");
            return;
        };
        let source = [0.5, 0.5, 0.5, 1.0];
        let shoot = |adjust: [f32; 2]| {
            let mut quad = flat_quad(source, 0);
            quad.adjust = adjust;
            render_pixel(
                &device,
                &queue,
                &atlas,
                &pipeline,
                quad,
                full_target_camera(),
            )
        };

        let neutral = shoot(QuadInstance::NEUTRAL_ADJUST);
        let brighter = shoot([0.4, 0.0]);
        let darker = shoot([-0.4, 0.0]);
        assert!(
            brighter[0] > neutral[0] + 40 && darker[0] + 40 < neutral[0],
            "brightness ต้องขยับ pixel จริง: มืด {darker:?} กลาง {neutral:?} สว่าง {brighter:?}"
        );

        // contrast ที่เทากลางพอดีต้องไม่ขยับ (เป็นจุดหมุนของสูตร) —
        // จึงต้องวัดกับสีที่ **ไม่ใช่** 0.5 ถึงจะเห็นผล
        let dim = [0.25, 0.25, 0.25, 1.0];
        let with_contrast = |adjust: [f32; 2]| {
            let mut quad = flat_quad(dim, 0);
            quad.adjust = adjust;
            render_pixel(
                &device,
                &queue,
                &atlas,
                &pipeline,
                quad,
                full_target_camera(),
            )
        };
        let plain = with_contrast(QuadInstance::NEUTRAL_ADJUST);
        let punchy = with_contrast([0.0, 0.6]);
        let flat = with_contrast([0.0, -0.6]);
        assert!(
            punchy[0] < plain[0] && flat[0] > plain[0],
            "สีที่มืดกว่ากลางต้องมืดลงเมื่อเพิ่ม contrast และจางลงเมื่อลด: \
             เพิ่ม {punchy:?} เดิม {plain:?} ลด {flat:?}"
        );
    }

    /// ★★ พิสูจน์ว่าการทวงที่คืนจาก `tint` **ได้ความละเอียดจริง** (docs/04 §3.5)
    ///
    /// เลือกค่าสองตัวที่ **ตกในถังเดียวกันของการแพ็กแบบ 8 บิตเดิม** (ก้าวละ 1/127):
    /// `25/127 ± 0.0035` → ทั้งคู่ปัดเป็น 25 เหมือนกัน = เดิมให้ pixel เดียวกันเป๊ะ
    ///
    /// แต่ระยะห่างจริงของมัน (0.007) กว้างกว่าหนึ่งขั้นของ framebuffer 8 บิต
    /// (1/255 ≈ 0.0039) → **ตาเห็นความต่างได้** ถ้าเก็บเป็น f32
    ///
    /// ★ เทสต์รุ่นแรกของข้อนี้ **ผ่านทั้งที่ยังแพ็ก 8 บิตอยู่** เพราะเลือกช่วงกว้างเกิน
    /// หนึ่งถัง — negative control จับได้ ถ้าไม่ได้ลองทำให้มันพัง เราจะเชื่อผิดว่า
    /// การเปลี่ยน layout ได้ผล ทั้งที่เทสต์ไม่ได้วัดสิ่งนั้นเลย (docs/08 §3.9 ข้อ 1)
    #[test]
    fn the_reclaimed_bytes_actually_buy_finer_brightness_steps() {
        let Some((device, queue, atlas, pipeline)) = pixel_harness() else {
            eprintln!("ข้าม: ไม่มี GPU adapter");
            return;
        };
        let source = [0.5, 0.5, 0.5, 1.0];
        let shoot = |brightness: f32| {
            let mut quad = flat_quad(source, 0);
            quad.adjust = [brightness, 0.0];
            render_pixel(
                &device,
                &queue,
                &atlas,
                &pipeline,
                quad,
                full_target_camera(),
            )[0]
        };

        // จุดกึ่งกลางของถังที่ 25 ในการแพ็กแบบเดิม
        let bucket = 25.0 / 127.0;
        let low = shoot(bucket - 0.0035);
        let high = shoot(bucket + 0.0035);

        assert_ne!(
            low, high,
            "สองค่านี้เคยตกถังเดียวกันตอนแพ็ก 8 บิต — ถ้ายังให้ pixel เดียวกัน              แปลว่าเปลี่ยน layout แล้วแต่ไม่ได้ความละเอียดกลับมา"
        );
        assert!(high > low, "ค่าที่สูงกว่าต้องสว่างกว่า: {low} vs {high}");
    }

    /// `tint` ที่เป็นไบต์แล้วต้องยังให้สีเดิมกลับมาในระดับที่ตาแยกไม่ออก
    #[test]
    fn packing_the_tint_into_bytes_does_not_shift_the_colour() {
        let Some((device, queue, atlas, pipeline)) = pixel_harness() else {
            eprintln!("ข้าม: ไม่มี GPU adapter");
            return;
        };
        for source in [
            [0.0, 0.25, 0.5, 1.0],
            [1.0, 0.75, 0.333, 1.0],
            [0.125, 0.875, 0.625, 1.0],
        ] {
            let got = render_pixel(
                &device,
                &queue,
                &atlas,
                &pipeline,
                flat_quad(source, 0),
                full_target_camera(),
            );
            for (channel, want) in got[..3].iter().zip(source) {
                let want = (want * 255.0_f32).round() as u8;
                assert!(
                    channel.abs_diff(want) <= 1,
                    "{source:?}: ได้ {got:?} คลาดจาก {want} เกินหนึ่งขั้น"
                );
            }
        }
    }

    /// ★ grayscale ระดับ board มาจาก **uniform ตัวเดียว** — instance ไม่เปลี่ยนเลย
    ///
    /// เทสต์นี้ยิง quad **ตัวเดิมเป๊ะ** สองครั้ง ต่างกันแค่ค่าใน uniform
    /// ซึ่งเป็นหลักฐานตรงว่าปุ่ม `G` ไม่ต้องแตะ instance buffer หรือ texture
    #[test]
    fn the_board_wide_grayscale_uniform_changes_pixels_without_touching_instances() {
        let Some((device, queue, atlas, pipeline)) = pixel_harness() else {
            eprintln!("ข้าม: ไม่มี GPU adapter");
            return;
        };
        let source = [0.9, 0.2, 0.1, 1.0];
        let quad = flat_quad(source, 0);

        let colour = render_pixel(
            &device,
            &queue,
            &atlas,
            &pipeline,
            quad,
            full_target_camera().with_grayscale(false),
        );
        let gray = render_pixel(
            &device,
            &queue,
            &atlas,
            &pipeline,
            quad, // ★ instance เดิมเป๊ะ ไม่ได้แก้อะไรเลย
            full_target_camera().with_grayscale(true),
        );

        assert!(colour[0] > colour[1] + 40, "ภาพสีต้องยังเป็นสี: {colour:?}");
        assert!(
            gray[0].abs_diff(gray[1]) <= 2 && gray[1].abs_diff(gray[2]) <= 2,
            "เปิด grayscale แล้วสามช่องต้องเท่ากัน: {gray:?}"
        );
        // Rec. 709 ของ (0.9, 0.2, 0.1) ≈ 0.34 — ไม่ใช่ค่าเฉลี่ยธรรมดา (0.4)
        // ข้อนี้จับได้ถ้ามีใครเปลี่ยนไปใช้ค่าเฉลี่ย ซึ่งนักวาดจะเห็นความต่างทันที
        let want = (0.2126 * 0.9 + 0.7152 * 0.2 + 0.0722 * 0.1) * 255.0;
        assert!(
            f32::from(gray[0]) - want < 4.0 && want - f32::from(gray[0]) < 4.0,
            "ต้องใช้ luminance ของ Rec. 709 (≈{want:.0}) ไม่ใช่ค่าเฉลี่ย: ได้ {gray:?}"
        );
    }

    /// ธง grayscale ของ **ภาพใบเดียว** ต้องทำงานแยกจากสวิตช์ระดับ board
    #[test]
    fn the_per_item_grayscale_flag_works_on_its_own() {
        let Some((device, queue, atlas, pipeline)) = pixel_harness() else {
            eprintln!("ข้าม: ไม่มี GPU adapter");
            return;
        };
        let source = [0.9, 0.2, 0.1, 1.0];
        let flagged = render_pixel(
            &device,
            &queue,
            &atlas,
            &pipeline,
            flat_quad(source, crate::instance::flags::GRAYSCALE),
            full_target_camera().with_grayscale(false),
        );
        assert!(
            flagged[0].abs_diff(flagged[1]) <= 2,
            "ธงของภาพเองต้องพอแล้ว แม้สวิตช์ระดับ board ปิดอยู่: {flagged:?}"
        );
    }

    /// invert ต้องกลับค่าจริง ๆ ไม่ใช่แค่ตั้งธงทิ้งไว้
    #[test]
    fn invert_flips_the_channels_it_is_given() {
        let Some((device, queue, atlas, pipeline)) = pixel_harness() else {
            eprintln!("ข้าม: ไม่มี GPU adapter");
            return;
        };
        let source = [0.8, 0.8, 0.8, 1.0];
        let got = render_pixel(
            &device,
            &queue,
            &atlas,
            &pipeline,
            flat_quad(source, crate::instance::flags::INVERT),
            full_target_camera(),
        );
        let want = ((1.0_f32 - 0.8) * 255.0).round() as u8;
        assert!(
            got[0].abs_diff(want) <= 2,
            "0.8 กลับค่าต้องได้ราว ๆ {want}: ได้ {got:?}"
        );
    }
}
