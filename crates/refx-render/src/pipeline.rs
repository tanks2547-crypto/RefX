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
    /// tx, ty แล้วเว้นอีก 2 ช่องให้ครบ 16 ไบต์
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
                visibility: wgpu::ShaderStages::VERTEX,
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
}
