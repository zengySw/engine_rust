struct Camera {
    view_proj: mat4x4<f32>,
    time_of_day: vec4<f32>, // x = 0..1
    cam_pos: vec4<f32>,     // xyz = camera world pos
    lighting: vec4<f32>,    // x=ambient_boost, y=sun_softness, z=fog_density, w=exposure
    rt_params: vec4<f32>,   // x=enabled, y=max_steps, z=max_dist, w=step_len
    parallax: vec4<f32>,    // x=parallax strength
}

@group(0) @binding(0)
var<uniform> cam: Camera;

@group(1) @binding(0)
var block_tex: texture_2d_array<f32>;
@group(1) @binding(1)
var block_sampler: sampler;
@group(1) @binding(2)
var parallax_tex: texture_2d_array<f32>;

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
    @location(4) world_pos: vec3<f32>,
}

@vertex
fn vs_main(v: VertIn) -> VertOut {
    var out: VertOut;
    out.clip_pos = cam.view_proj * vec4<f32>(v.pos, 1.0);
    out.normal = v.normal;
    out.tex_idx = v.tex_idx;
    out.world_y = v.pos.y;
    out.uv = v.uv;
    out.world_pos = v.pos;
    return out;
}

fn trace_shadow_stub(origin: vec3<f32>, dir: vec3<f32>) -> f32 {
    if cam.rt_params.x < 0.5 {
        return 1.0;
    }

    let max_steps = i32(clamp(cam.rt_params.y, 1.0, 128.0));
    let max_dist = max(cam.rt_params.z, 1.0);
    let step_len = max(cam.rt_params.w, 0.25);

    var t = step_len;
    var attenuation = 1.0;
    for (var i = 0; i < 128; i = i + 1) {
        if i >= max_steps || t > max_dist {
            break;
        }

        let p = origin + dir * t;
        // TODO: replace with real voxel/scene occupancy query.
        attenuation = min(attenuation, 1.0 - (t / max_dist + p.y * 0.0) * 0.12);
        t = t + step_len;
    }

    return clamp(attenuation, 0.0, 1.0);
}

fn minecraft_face_shade(n: vec3<f32>) -> f32 {
    // Classic voxel face brightness:
    // top brightest, bottom darkest, one side axis darker than the other.
    if n.y > 0.5 {
        return 1.0;
    }
    if n.y < -0.5 {
        return 0.50;
    }
    if abs(n.x) > 0.5 {
        return 0.68;
    }
    return 0.82;
}

fn tonemap_aces(x: vec3<f32>) -> vec3<f32> {
    let a = 2.51;
    let b = 0.03;
    let c = 2.43;
    let d = 0.59;
    let e = 0.14;
    return clamp((x * (a * x + b)) / (x * (c * x + d) + e), vec3<f32>(0.0), vec3<f32>(1.0));
}

fn dim_texture_color(color: vec3<f32>) -> vec3<f32> {
    let luma = dot(color, vec3<f32>(0.2126, 0.7152, 0.0722));
    let desat = mix(vec3<f32>(luma), color, 0.96);
    return desat * 0.95;
}

fn face_uv_view_dir(view_dir: vec3<f32>, n: vec3<f32>) -> vec2<f32> {
    if n.x > 0.5 {
        return vec2<f32>(view_dir.z, -view_dir.y);
    }
    if n.x < -0.5 {
        return vec2<f32>(-view_dir.z, -view_dir.y);
    }
    if n.y > 0.5 {
        return vec2<f32>(view_dir.x, -view_dir.z);
    }
    if n.y < -0.5 {
        return vec2<f32>(view_dir.x, view_dir.z);
    }
    if n.z > 0.5 {
        return vec2<f32>(-view_dir.x, -view_dir.y);
    }
    return vec2<f32>(view_dir.x, -view_dir.y);
}

@fragment
fn fs_main(in: VertOut) -> @location(0) vec4<f32> {
    let t = cam.time_of_day.x;
    let sun_angle = t * 6.283185;
    let sun_height = sin(sun_angle);
    let sun_up = max(sun_height, 0.0);
    let sun_dir = normalize(vec3<f32>(cos(sun_angle), sun_up, 0.35));
    let day = smoothstep(-0.08, 0.14, sun_height);
    let night = 1.0 - day;
    let twilight = 1.0 - smoothstep(0.02, 0.42, abs(sun_height));

    let n = normalize(in.normal);
    let shadow = trace_shadow_stub(in.world_pos + n * 0.05, sun_dir);
    let moon_dir = normalize(vec3<f32>(-cos(sun_angle), max(-sun_height, 0.0), -0.35));

    let view_to_cam = normalize(cam.cam_pos.xyz - in.world_pos);
    let h = textureSample(parallax_tex, block_sampler, in.uv, i32(in.tex_idx)).r;
    let grazing = 1.0 - abs(dot(view_to_cam, n));
    let parallax_amount = (h - 0.5) * max(cam.parallax.x, 0.0) * (0.2 + grazing * 0.8);
    let uv = clamp(in.uv + face_uv_view_dir(view_to_cam, n) * parallax_amount, vec2<f32>(0.001), vec2<f32>(0.999));

    let sampled = textureSample(block_tex, block_sampler, uv, i32(in.tex_idx));
    if sampled.a < 0.1 {
        discard;
    }
    let albedo = dim_texture_color(sampled.rgb);

    let up = clamp(n.y * 0.5 + 0.5, 0.0, 1.0);
    let face_base = minecraft_face_shade(n);
    let hemi_soft = mix(0.54, 1.0, up);
    let face_light = mix(face_base, hemi_soft, cam.lighting.y * 0.45);

    let sun_wrap = mix(0.06, 0.32, cam.lighting.y);
    let ndl = clamp((dot(n, sun_dir) + sun_wrap) / (1.0 + sun_wrap), 0.0, 1.0);
    let nml = max(dot(n, moon_dir), 0.0);
    let underground = clamp((40.0 - in.world_pos.y) / 48.0, 0.0, 1.0);
    let cave_dim = mix(1.0, 0.62, underground);
    let ambient_scalar = mix(0.18, 0.68, day) * cam.lighting.x * cave_dim;
    let sky_fill = mix(0.02, 0.14, day)
        * (0.38 + 0.62 * up)
        * mix(1.0, 0.74, underground);
    let sun_scalar = (0.28 + 0.86 * pow(ndl, 1.22)) * day * shadow;
    let moon_scalar = (0.02 + 0.10 * pow(nml, 1.35)) * night * 0.58;
    let light_scalar = clamp(face_light * (ambient_scalar + sun_scalar + moon_scalar) + sky_fill, 0.035, 1.65);

    var sky_color = mix(vec3<f32>(0.015, 0.025, 0.060), vec3<f32>(0.56, 0.80, 1.00), day);
    sky_color = mix(sky_color, vec3<f32>(1.00, 0.56, 0.26), twilight * 0.45);

    let dist = abs(in.clip_pos.w);
    let fog_density = mix(0.0019, 0.00072, day) * max(cam.lighting.z, 0.15);
    let fog = clamp(1.0 - exp(-dist * fog_density), 0.0, 1.0);

    let tint = mix(vec3<f32>(0.88, 0.93, 1.05), vec3<f32>(1.0, 1.0, 1.0), day);
    let lit_linear = max(albedo * light_scalar * tint * cam.lighting.w, vec3<f32>(0.0));
    let lit_boosted = lit_linear * mix(0.96, 1.08, day);
    let lit = pow(tonemap_aces(lit_boosted), vec3<f32>(1.0 / 2.2));
    let final_color = mix(lit, sky_color, fog * mix(0.94, 0.76, day));
    return vec4<f32>(final_color, 1.0);
}

fn smoothstep(edge0: f32, edge1: f32, x: f32) -> f32 {
    let t = clamp((x - edge0) / (edge1 - edge0), 0.0, 1.0);
    return t * t * (3.0 - 2.0 * t);
}
