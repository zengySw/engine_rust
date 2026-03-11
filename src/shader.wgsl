struct Camera {
    view_proj: mat4x4<f32>,
    time_of_day: vec4<f32>, // x = 0..1
}

@group(0) @binding(0)
var<uniform> cam: Camera;

@group(1) @binding(0)
var block_tex: texture_2d_array<f32>;
@group(1) @binding(1)
var block_sampler: sampler;

struct VertIn {
    @location(0) pos: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) tex_idx: u32,
    @location(3) uv: vec2<f32>,
}

struct VertOut {
    @builtin(position) clip_pos: vec4<f32>,
    @location(0) normal: vec3<f32>,
    @location(1) tex_idx: u32,
    @location(2) world_y: f32,
    @location(3) uv: vec2<f32>,
}

@vertex
fn vs_main(v: VertIn) -> VertOut {
    var out: VertOut;
    out.clip_pos = cam.view_proj * vec4<f32>(v.pos, 1.0);
    out.normal = v.normal;
    out.tex_idx = v.tex_idx;
    out.world_y = v.pos.y;
    out.uv = v.uv;
    return out;
}

@fragment
fn fs_main(in: VertOut) -> @location(0) vec4<f32> {
    let t = cam.time_of_day.x;
    let sun_angle = t * 6.283185;
    let sun_height = sin(sun_angle);
    let sun_dir = normalize(vec3<f32>(cos(sun_angle), max(sun_height, 0.0), 0.4));
    let day = smoothstep(-0.05, 0.15, sun_height);
    let night = 1.0 - day;
    let sun_term = max(dot(in.normal, sun_dir), 0.0) * day;

    let face_shade = select(
        select(0.80, 0.55, in.normal.y < -0.5),
        1.00,
        in.normal.y > 0.5
    );
    let skylight = mix(0.08, 1.00, day);
    let light = skylight * face_shade + sun_term * 0.25;

    let sampled = textureSample(block_tex, block_sampler, in.uv, i32(in.tex_idx));
    if sampled.a < 0.1 {
        discard;
    }
    let color = sampled.rgb;

    let sky_day = vec3(0.53, 0.81, 0.98);
    let sky_night = vec3(0.02, 0.03, 0.08);
    let sky_dusk = vec3(0.95, 0.55, 0.25);
    let dusk = smoothstep(0.25, 0.65, 1.0 - abs(sun_height)) * night;
    var sky_color = mix(sky_night, sky_day, day);
    sky_color = mix(sky_color, sky_dusk, dusk);

    let fog_start = 180.0;
    let fog_end = 320.0;
    let dist = abs(in.clip_pos.w);
    let fog = clamp((dist - fog_start) / (fog_end - fog_start), 0.0, 1.0);

    let night_tint = vec3(0.55, 0.65, 1.0);
    let tint = mix(night_tint, vec3(1.0, 1.0, 1.0), day);
    let lit = pow(color * light * tint, vec3(1.0 / 1.2));
    let final_color = mix(lit, sky_color, fog);
    return vec4<f32>(final_color, 1.0);
}

fn smoothstep(edge0: f32, edge1: f32, x: f32) -> f32 {
    let t = clamp((x - edge0) / (edge1 - edge0), 0.0, 1.0);
    return t * t * (3.0 - 2.0 * t);
}
