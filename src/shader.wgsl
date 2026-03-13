struct Camera {
    view_proj: mat4x4<f32>,
    time_of_day: vec4<f32>, // x = 0..1
    lighting: vec4<f32>,    // x=ambient_boost, y=sun_softness, z=fog_density, w=exposure
    rt_params: vec4<f32>,   // x=enabled, y=max_steps, z=max_dist, w=step_len
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

fn tonemap_aces(x: vec3<f32>) -> vec3<f32> {
    let a = 2.51;
    let b = 0.03;
    let c = 2.43;
    let d = 0.59;
    let e = 0.14;
    return clamp((x * (a * x + b)) / (x * (c * x + d) + e), vec3(0.0), vec3(1.0));
}

fn saturate_color(color: vec3<f32>, sat: f32) -> vec3<f32> {
    let luma = dot(color, vec3<f32>(0.2126, 0.7152, 0.0722));
    return mix(vec3<f32>(luma), color, sat);
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
    let up = clamp(n.y * 0.5 + 0.5, 0.0, 1.0);
    let axis_shade = abs(n.y) * 1.00 + abs(n.x) * 0.82 + abs(n.z) * 0.90;

    let sky_ambient_day = vec3<f32>(0.40, 0.48, 0.62) * cam.lighting.x;
    let sky_ambient_night = vec3<f32>(0.05, 0.08, 0.16);
    let ground_bounce_day = vec3<f32>(0.12, 0.10, 0.07);
    let ground_bounce_night = vec3<f32>(0.02, 0.02, 0.03);
    let sky_ambient = mix(sky_ambient_night, sky_ambient_day, day);
    let ground_bounce = mix(ground_bounce_night, ground_bounce_day, day);
    var ambient = mix(ground_bounce, sky_ambient, up);
    ambient = ambient + vec3<f32>(0.14, 0.10, 0.08) * twilight * 0.22;

    let wrap = clamp((dot(n, sun_dir) + cam.lighting.y) / (1.0 + cam.lighting.y), 0.0, 1.0);
    let shadow = trace_shadow_stub(in.world_pos + n * 0.05, sun_dir);
    let sun_elevation = smoothstep(0.0, 0.45, sun_up);
    let sun_color = mix(vec3<f32>(1.35, 0.80, 0.45), vec3<f32>(1.05, 1.00, 0.95), sun_elevation);
    let sun_term = sun_color * pow(wrap, 1.25) * day * shadow * mix(0.75, 1.0, sun_elevation);

    let moon_dir = normalize(vec3<f32>(-cos(sun_angle), max(-sun_height, 0.0), -0.35));
    let moon_term = vec3<f32>(0.11, 0.14, 0.24) * pow(max(dot(n, moon_dir), 0.0), 1.4) * night * 0.75;

    let sampled = textureSample(block_tex, block_sampler, in.uv, i32(in.tex_idx));
    if sampled.a < 0.1 {
        discard;
    }
    let albedo = sampled.rgb;
    let skylight = mix(0.28, 1.0, day);
    let light = (ambient + sun_term + moon_term) * skylight * axis_shade * mix(0.92, 1.04, up);

    var sky_color = mix(vec3<f32>(0.015, 0.025, 0.060), vec3<f32>(0.52, 0.77, 0.97), day);
    sky_color = mix(sky_color, vec3<f32>(1.00, 0.54, 0.23), twilight * 0.55);

    let dist = abs(in.clip_pos.w);
    let low_alt = clamp((40.0 - in.world_pos.y) / 70.0, 0.0, 1.0);
    let fog_density = mix(0.0028, 0.00135, day) * max(cam.lighting.z, 0.25);
    let fog = clamp(1.0 - exp(-dist * fog_density * (1.0 + low_alt * 0.75)), 0.0, 1.0);

    let night_tint = vec3<f32>(0.62, 0.73, 1.02);
    let tint = mix(night_tint, vec3<f32>(1.0, 1.0, 1.0), day);
    let base = max(albedo * light * tint * cam.lighting.w, vec3<f32>(0.0));
    let sat = mix(0.88, 1.00, day);
    let lit = pow(tonemap_aces(saturate_color(base, sat)), vec3<f32>(1.0 / 2.2));
    let final_color = mix(lit, sky_color, fog);
    return vec4<f32>(final_color, 1.0);
}

fn smoothstep(edge0: f32, edge1: f32, x: f32) -> f32 {
    let t = clamp((x - edge0) / (edge1 - edge0), 0.0, 1.0);
    return t * t * (3.0 - 2.0 * t);
}
