// quad.wgsl — instanced quad ของ RefX
//
// vertex   : unit quad → affine transform → world → clip
// fragment : สี tint + flags (grayscale / invert / selection outline)
//
// P1-5 จะเพิ่ม texture array (atlas) เข้ามาที่ fragment
// ตอนนี้ทุก instance วาดเป็นสี่เหลี่ยมทึบ (bit PLACEHOLDER)
//
// spec: docs/04-rendering.md §9

// ---- bit ของ QuadInstance.flags (ต้องตรงกับ instance.rs) ----
const FLAG_GRAYSCALE:   u32 = 1u;
const FLAG_INVERT:      u32 = 2u;
const FLAG_SELECTED:    u32 = 4u;
const FLAG_PLACEHOLDER: u32 = 8u;

// luminance ของ Rec. 709 — ไม่ใช่ค่าเฉลี่ยธรรมดา
// ผลต่างเห็นชัดมากเวลานักวาดใช้เช็ค value ของภาพ
const LUMA: vec3<f32> = vec3<f32>(0.2126, 0.7152, 0.0722);

struct Camera {
    // affine world→clip เก็บเป็น 2 vec4 เพื่อให้ align 16 ไบต์
    // a, b, c, d
    view_a: vec4<f32>,
    // tx, ty, (ว่าง 2 ช่อง)
    view_b: vec4<f32>,
};

@group(0) @binding(0) var<uniform> camera: Camera;

// thumbnail atlas — 1 bind group เดียวสำหรับทุกภาพบน board (docs/04 §4)
@group(1) @binding(0) var atlas: texture_2d_array<f32>;
@group(1) @binding(1) var atlas_sampler: sampler;

struct VertexInput {
    // มุมของ unit quad: (0,0) (1,0) (0,1) (1,1)
    @location(6) corner: vec2<f32>,
};

struct InstanceInput {
    @location(0) transform_a: vec4<f32>,  // a, b, c, d
    @location(1) transform_b: vec2<f32>,  // tx, ty
    @location(2) uv_rect:     vec4<f32>,
    @location(3) tint:        vec4<f32>,
    @location(4) layer:       u32,
    @location(5) flags:       u32,
};

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) uv:    vec2<f32>,
    @location(1) tint:  vec4<f32>,
    // ต้อง flat: ค่าเดียวกันทั้ง primitive ห้าม interpolate
    @location(2) @interpolate(flat) flags: u32,
    @location(3) @interpolate(flat) layer: u32,
    // ตำแหน่งภายใน quad (0..1) ใช้วาดกรอบเลือก
    @location(4) local: vec2<f32>,
};

/// คูณจุดด้วย affine 2×3
fn apply_affine(a: vec4<f32>, b: vec2<f32>, p: vec2<f32>) -> vec2<f32> {
    return vec2<f32>(
        a.x * p.x + a.z * p.y + b.x,
        a.y * p.x + a.w * p.y + b.y,
    );
}

@vertex
fn vs_main(vertex: VertexInput, instance: InstanceInput) -> VertexOutput {
    var out: VertexOutput;

    let world = apply_affine(instance.transform_a, instance.transform_b, vertex.corner);
    let clip = apply_affine(camera.view_a, camera.view_b.xy, world);

    out.clip_position = vec4<f32>(clip, 0.0, 1.0);
    // interpolate uv ตาม uv_rect ของ instance
    out.uv = mix(instance.uv_rect.xy, instance.uv_rect.zw, vertex.corner);
    out.tint = instance.tint;
    out.flags = instance.flags;
    out.layer = instance.layer;
    out.local = vertex.corner;
    return out;
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    // ภาพที่ยังไม่มี texture วาดเป็นสี่เหลี่ยมทึบสี tint (สีเด่นจาก cache)
    // ห้ามรอ ห้ามข้าม — ผู้ใช้ต้องเห็น layout ทันที (docs/04 §8)
    let is_placeholder = (in.flags & FLAG_PLACEHOLDER) != 0u;
    let sampled = textureSample(atlas, atlas_sampler, in.uv, in.layer);
    // select() แทน if เพื่อไม่ให้ warp แตกสาย (docs/04 §9)
    var color = select(sampled * in.tint, in.tint, is_placeholder);

    // ใช้ select() แทน if เพื่อไม่ให้ warp แตกสาย (docs/04 §9)
    let gray = vec4<f32>(vec3<f32>(dot(color.rgb, LUMA)), color.a);
    color = select(color, gray, (in.flags & FLAG_GRAYSCALE) != 0u);

    let inverted = vec4<f32>(1.0 - color.rgb, color.a);
    color = select(color, inverted, (in.flags & FLAG_INVERT) != 0u);

    // กรอบเลือกวาดใน fragment — ไม่ต้องเพิ่ม draw call
    let edge = min(min(in.local.x, 1.0 - in.local.x), min(in.local.y, 1.0 - in.local.y));
    let on_border = edge < 0.02;
    let selected = (in.flags & FLAG_SELECTED) != 0u;
    color = select(color, vec4<f32>(0.20, 0.60, 1.0, 1.0), selected && on_border);

    return color;
}
