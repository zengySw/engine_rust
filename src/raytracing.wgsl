struct RtUniform {
    inv_view_proj: mat4x4<f32>,
    cam_day: vec4<f32>,    // xyz = camera position, w = day_time
    world_min: vec4<f32>,  // xyz = voxel volume world origin
    world_size: vec4<f32>, // xyz = voxel volume size
    settings: vec4<f32>,   // x=max_primary_steps, y=max_shadow_steps, z=max_shadow_dist, w=exposure
}

@group(0) @binding(0)
var<uniform> u: RtUniform;
@group(0) @binding(1)
var voxels: texture_3d<u32>;

struct VsOut {
    @builtin(position) clip_pos: vec4<f32>,
    @location(0) uv: vec2<f32>,
}

@vertex
fn vs_main(@builtin(vertex_index) vi: u32) -> VsOut {
    var out: VsOut;
    var pos = vec2<f32>(-1.0, -1.0);
    switch vi {
        case 0u: {
            pos = vec2<f32>(-1.0, -1.0);
        }
        case 1u: {
            pos = vec2<f32>(3.0, -1.0);
        }
        default: {
            pos = vec2<f32>(-1.0, 3.0);
        }
    }
    out.clip_pos = vec4<f32>(pos, 0.0, 1.0);
    out.uv = pos * 0.5 + vec2<f32>(0.5, 0.5);
    return out;
}

fn ray_box(ro: vec3<f32>, rd: vec3<f32>, bmin: vec3<f32>, bmax: vec3<f32>) -> vec2<f32> {
    let inv = 1.0 / rd;
    let t0 = (bmin - ro) * inv;
    let t1 = (bmax - ro) * inv;
    let tmin3 = min(t0, t1);
    let tmax3 = max(t0, t1);
    let t_near = max(max(tmin3.x, tmin3.y), tmin3.z);
    let t_far = min(min(tmax3.x, tmax3.y), tmax3.z);
    return vec2<f32>(t_near, t_far);
}

fn in_bounds(cell: vec3<i32>, size: vec3<i32>) -> bool {
    return all(cell >= vec3<i32>(0)) && all(cell < size);
}

fn voxel_id(cell: vec3<i32>) -> u32 {
    let size = vec3<i32>(i32(u.world_size.x), i32(u.world_size.y), i32(u.world_size.z));
    if !in_bounds(cell, size) {
        return 0u;
    }
    return textureLoad(voxels, cell, 0).r;
}

fn block_color(id: u32) -> vec3<f32> {
    switch id {
        case 1u: { return vec3<f32>(0.44, 0.72, 0.32); } // grass
        case 2u: { return vec3<f32>(0.52, 0.36, 0.22); } // dirt
        case 3u: { return vec3<f32>(0.58, 0.58, 0.60); } // stone
        case 4u: { return vec3<f32>(0.86, 0.78, 0.52); } // sand
        case 6u: { return vec3<f32>(0.18, 0.18, 0.20); } // bedrock
        case 7u: { return vec3<f32>(0.52, 0.36, 0.22); } // log
        case 8u: { return vec3<f32>(0.62, 0.47, 0.30); } // logBottom
        case 9u: { return vec3<f32>(0.34, 0.58, 0.30); } // leaves
        default: { return vec3<f32>(1.0, 0.0, 1.0); }
    }
}

fn trace_shadow(ro_world: vec3<f32>, dir_world: vec3<f32>, max_steps: i32, max_dist: f32) -> f32 {
    let ro_local = ro_world - u.world_min.xyz;
    var t = 0.8;
    let step_len = 0.9;
    for (var i = 0; i < 256; i = i + 1) {
        if i >= max_steps || t > max_dist {
            break;
        }
        let p = ro_local + dir_world * t;
        let c = vec3<i32>(floor(p));
        if voxel_id(c) > 0u {
            return 0.22;
        }
        t = t + step_len;
    }
    return 1.0;
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    let ndc = vec2<f32>(in.uv.x * 2.0 - 1.0, in.uv.y * 2.0 - 1.0);
    let far_h = u.inv_view_proj * vec4<f32>(ndc, 1.0, 1.0);
    let far_world = far_h.xyz / max(far_h.w, 1e-6);

    let ro_world = u.cam_day.xyz;
    var rd_world = normalize(far_world - ro_world);
    if abs(rd_world.x) < 1e-5 { rd_world.x = 1e-5; }
    if abs(rd_world.y) < 1e-5 { rd_world.y = 1e-5; }
    if abs(rd_world.z) < 1e-5 { rd_world.z = 1e-5; }
    let ro_local = ro_world - u.world_min.xyz;
    let size_f = u.world_size.xyz;

    let hit_range = ray_box(ro_local, rd_world, vec3<f32>(0.0), size_f);
    if hit_range.x > hit_range.y || hit_range.y < 0.0 {
        return vec4<f32>(sky_color(u.cam_day.w, rd_world.y), 1.0);
    }

    var t = max(hit_range.x, 0.0) + 0.0005;
    var p = ro_local + rd_world * t;
    var cell = vec3<i32>(floor(p));

    let step = vec3<i32>(select(vec3<i32>(-1), vec3<i32>(1), rd_world >= vec3<f32>(0.0)));
    let t_delta = abs(1.0 / rd_world);
    let next_boundary = vec3<f32>(cell) + select(vec3<f32>(0.0), vec3<f32>(1.0), rd_world >= vec3<f32>(0.0));
    var t_max = (next_boundary - p) / rd_world;

    let max_steps = i32(clamp(u.settings.x, 8.0, 4096.0));
    var hit = false;
    var hit_id = 0u;
    var normal = vec3<f32>(0.0, 1.0, 0.0);

    for (var i = 0; i < 4096; i = i + 1) {
        if i >= max_steps || t > hit_range.y {
            break;
        }

        hit_id = voxel_id(cell);
        if hit_id > 0u {
            hit = true;
            break;
        }

        if t_max.x < t_max.y {
            if t_max.x < t_max.z {
                t = t_max.x;
                t_max.x = t_max.x + t_delta.x;
                cell.x = cell.x + step.x;
                normal = vec3<f32>(-f32(step.x), 0.0, 0.0);
            } else {
                t = t_max.z;
                t_max.z = t_max.z + t_delta.z;
                cell.z = cell.z + step.z;
                normal = vec3<f32>(0.0, 0.0, -f32(step.z));
            }
        } else {
            if t_max.y < t_max.z {
                t = t_max.y;
                t_max.y = t_max.y + t_delta.y;
                cell.y = cell.y + step.y;
                normal = vec3<f32>(0.0, -f32(step.y), 0.0);
            } else {
                t = t_max.z;
                t_max.z = t_max.z + t_delta.z;
                cell.z = cell.z + step.z;
                normal = vec3<f32>(0.0, 0.0, -f32(step.z));
            }
        }
    }

    if !hit {
        return vec4<f32>(sky_color(u.cam_day.w, rd_world.y), 1.0);
    }

    let hit_world = ro_world + rd_world * t;
    let base = block_color(hit_id);

    let day_time = u.cam_day.w;
    let sun_angle = day_time * 6.28318530718;
    let sun_height = sin(sun_angle);
    let day = smoothstep(-0.07, 0.18, sun_height);
    let night = 1.0 - day;
    let sun_dir = normalize(vec3<f32>(cos(sun_angle), max(sun_height, 0.0), 0.35));
    let moon_dir = -sun_dir;

    let ndl = max(dot(normal, sun_dir), 0.0);
    let nml = max(dot(normal, moon_dir), 0.0);
    let shadow = trace_shadow(
        hit_world + normal * 0.04,
        sun_dir,
        i32(clamp(u.settings.y, 4.0, 256.0)),
        u.settings.z
    );

    let ambient = mix(vec3<f32>(0.07, 0.08, 0.11), vec3<f32>(0.38, 0.42, 0.48), day);
    let sun = vec3<f32>(1.00, 0.95, 0.86) * ndl * shadow * day;
    let moon = vec3<f32>(0.08, 0.11, 0.18) * nml * night * 0.6;
    let lit = base * (ambient + sun + moon);

    let dist = t;
    let fog_start = mix(68.0, 120.0, day);
    let fog_end = fog_start + 140.0;
    let fog = clamp((dist - fog_start) / (fog_end - fog_start), 0.0, 1.0);
    let sky = sky_color(day_time, rd_world.y);
    let color = mix(lit * u.settings.w, sky, fog);

    return vec4<f32>(pow(color, vec3<f32>(1.0 / 1.15)), 1.0);
}

fn sky_color(time: f32, ray_y: f32) -> vec3<f32> {
    let angle = time * 6.28318530718;
    let sun_y = sin(angle);
    let day = smoothstep(-0.05, 0.15, sun_y);
    let night = 1.0 - day;

    let day_sky = vec3<f32>(0.55, 0.79, 0.98);
    let night_sky = vec3<f32>(0.02, 0.03, 0.08);
    let dusk_sky = vec3<f32>(0.95, 0.54, 0.25);
    let dusk = smoothstep(0.20, 0.65, 1.0 - abs(sun_y)) * night;

    var sky = mix(night_sky, day_sky, day);
    sky = mix(sky, dusk_sky, dusk);
    let horizon = smoothstep(-0.3, 0.25, ray_y);
    return mix(sky * 0.8, sky, horizon);
}

fn smoothstep(edge0: f32, edge1: f32, x: f32) -> f32 {
    let t = clamp((x - edge0) / (edge1 - edge0), 0.0, 1.0);
    return t * t * (3.0 - 2.0 * t);
}
