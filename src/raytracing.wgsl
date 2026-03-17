struct RtUniform {
    inv_view_proj: mat4x4<f32>,
    cam_day: vec4<f32>,    // xyz = camera position, w = day_time
    world_min: vec4<f32>,  // xyz = voxel volume world origin
    world_size: vec4<f32>, // xyz = voxel volume size
    settings: vec4<f32>,   // x=max_primary_steps, y=max_shadow_steps, z=max_shadow_dist, w=exposure
    weather: vec4<f32>,    // x=rain_strength, y=rain_time, z=surface_wetness
}

@group(0) @binding(0)
var<uniform> u: RtUniform;
@group(0) @binding(1)
var voxels: texture_3d<u32>;
@group(0) @binding(2)
var block_tex: texture_2d_array<f32>;
@group(0) @binding(3)
var block_sampler: sampler;

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

fn face_uv(local: vec3<f32>, normal: vec3<f32>) -> vec2<f32> {
    // Matches UV orientation used by chunk mesh generation.
    if normal.x > 0.5 {
        return vec2<f32>(local.z, 1.0 - local.y);
    }
    if normal.x < -0.5 {
        return vec2<f32>(1.0 - local.z, 1.0 - local.y);
    }
    if normal.y > 0.5 {
        return vec2<f32>(local.x, 1.0 - local.z);
    }
    if normal.y < -0.5 {
        return vec2<f32>(local.x, local.z);
    }
    if normal.z > 0.5 {
        return vec2<f32>(1.0 - local.x, 1.0 - local.y);
    }
    return vec2<f32>(local.x, 1.0 - local.y);
}

fn face_texture_layer(base_layer: u32, normal: vec3<f32>) -> i32 {
    // Keep side/top selection parity with raster path:
    // grass: top=grass, all others=dirt
    // log: top/bottom=logBottom, sides=log
    // wood(planks): top/bottom=logBottom, sides=wood
    // workbench: top/bottom=workbench_top, +/-Z=workbench_front, +/-X=workbench_side
    // furnace: top/bottom=furnace_top, +Z=furnace_front, others=furnace_side
    if base_layer == 1u {
        if normal.y > 0.5 {
            return 1;
        }
        return 2;
    }
    if base_layer == 7u {
        if abs(normal.y) > 0.5 {
            return 8;
        }
        return 7;
    }
    if base_layer == 17u {
        if abs(normal.y) > 0.5 {
            return 8;
        }
        return 17;
    }
    if base_layer == 16u {
        if abs(normal.y) > 0.5 {
            return 19;
        }
        if abs(normal.z) > 0.5 {
            return 20;
        }
        return 16;
    }
    if base_layer == 21u {
        if abs(normal.y) > 0.5 {
            return 22;
        }
        if normal.z > 0.5 {
            return 23;
        }
        return 21;
    }
    return i32(base_layer);
}

fn minecraft_face_shade(n: vec3<f32>) -> f32 {
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

fn tonemap_reinhard(color: vec3<f32>) -> vec3<f32> {
    return color / (vec3<f32>(1.0) + color);
}

fn dim_texture_color(color: vec3<f32>) -> vec3<f32> {
    let luma = dot(color, vec3<f32>(0.2126, 0.7152, 0.0722));
    let desat = mix(vec3<f32>(luma), color, 0.96);
    return desat * 0.95;
}

fn hash12(p: vec2<f32>) -> f32 {
    let h = sin(dot(p, vec2<f32>(127.1, 311.7))) * 43758.5453;
    return fract(h);
}

fn puddle_pattern(xz: vec2<f32>, rain: f32) -> f32 {
    let p0 = floor(xz * 1.12);
    let p1 = floor(xz * 2.73 + vec2<f32>(17.0, 31.0));
    let p2 = floor(xz * 0.58 + vec2<f32>(-11.0, 7.0));
    let n = hash12(p0) * 0.56 + hash12(p1) * 0.30 + hash12(p2) * 0.14;
    let mask = smoothstep(0.69, 0.86, n);
    let rain_fill = smoothstep(0.15, 0.92, rain);
    return mask * rain_fill;
}

fn is_puddle_candidate(id: u32) -> bool {
    switch id {
        case 1u: { return true; }   // grass
        case 2u: { return true; }   // dirt
        case 3u: { return true; }   // stone
        case 4u: { return true; }   // sand
        case 10u: { return true; }  // coal ore
        case 11u: { return true; }  // iron ore
        case 12u: { return true; }  // copper ore
        case 13u: { return true; }  // farmland dry
        case 14u: { return true; }  // farmland wet
        default: { return false; }
    }
}

fn trace_reflection_color(ro_world: vec3<f32>, rd_world_in: vec3<f32>, day_time: f32) -> vec3<f32> {
    var rd_world = normalize(rd_world_in);
    if abs(rd_world.x) < 1e-5 {
        rd_world.x = select(-1e-5, 1e-5, rd_world.x >= 0.0);
    }
    if abs(rd_world.y) < 1e-5 {
        rd_world.y = select(-1e-5, 1e-5, rd_world.y >= 0.0);
    }
    if abs(rd_world.z) < 1e-5 {
        rd_world.z = select(-1e-5, 1e-5, rd_world.z >= 0.0);
    }

    let ro_local = ro_world - u.world_min.xyz;
    let hit_range = ray_box(ro_local, rd_world, vec3<f32>(0.0), u.world_size.xyz);
    if hit_range.x > hit_range.y || hit_range.y < 0.0 {
        return sky_color(day_time, rd_world.y);
    }

    var t = max(hit_range.x, 0.0) + 0.0009;
    var p = ro_local + rd_world * t;
    var cell = vec3<i32>(floor(p));

    let step = vec3<i32>(select(vec3<i32>(-1), vec3<i32>(1), rd_world >= vec3<f32>(0.0)));
    let t_delta = abs(1.0 / rd_world);
    let next_boundary = vec3<f32>(cell) + select(vec3<f32>(0.0), vec3<f32>(1.0), rd_world >= vec3<f32>(0.0));
    var t_max = (next_boundary - p) / rd_world;

    var hit = false;
    var hit_id = 0u;
    var normal = vec3<f32>(0.0, 1.0, 0.0);

    for (var i = 0; i < 192; i = i + 1) {
        if t > hit_range.y {
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
        return sky_color(day_time, rd_world.y);
    }

    let hit_local_world = ro_local + rd_world * t;
    let local = clamp(
        hit_local_world - vec3<f32>(cell),
        vec3<f32>(0.0),
        vec3<f32>(1.0)
    );
    let uv = clamp(face_uv(local, normal), vec2<f32>(0.001), vec2<f32>(0.999));
    let layer = face_texture_layer(hit_id, normal);
    let texel = textureSample(block_tex, block_sampler, uv, layer);
    let base = select(vec3<f32>(1.0, 0.0, 1.0), dim_texture_color(texel.rgb), texel.a > 0.01);

    let sun_angle = day_time * 6.28318530718;
    let sun_height = sin(sun_angle);
    let day = smoothstep(-0.07, 0.18, sun_height);
    let sun_dir = normalize(vec3<f32>(cos(sun_angle), max(sun_height, 0.0), 0.35));
    let face_light = minecraft_face_shade(normal);
    let ndl = max(dot(normal, sun_dir), 0.0);
    let light_scalar = clamp(face_light * (mix(0.16, 0.86, day) + 0.45 * ndl * day), 0.03, 1.35);

    let tint = mix(vec3<f32>(0.78, 0.85, 1.06), vec3<f32>(1.0, 1.0, 1.0), day);
    let lit_linear = max(base * light_scalar * tint, vec3<f32>(0.0));
    return pow(tonemap_reinhard(lit_linear), vec3<f32>(1.0 / 2.1));
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
    let hit_local_world = ro_local + rd_world * t;
    let local = clamp(
        hit_local_world - vec3<f32>(cell),
        vec3<f32>(0.0),
        vec3<f32>(1.0)
    );
    let uv = clamp(face_uv(local, normal), vec2<f32>(0.001), vec2<f32>(0.999));
    let layer = face_texture_layer(hit_id, normal);
    let texel = textureSample(block_tex, block_sampler, uv, layer);
    let base = select(vec3<f32>(1.0, 0.0, 1.0), dim_texture_color(texel.rgb), texel.a > 0.01);

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

    let face_base = minecraft_face_shade(normal);
    let hemi_soft = mix(0.56, 1.0, clamp(normal.y * 0.5 + 0.5, 0.0, 1.0));
    let face_light = mix(face_base, hemi_soft, 0.35);
    let underground = clamp((40.0 - hit_world.y) / 48.0, 0.0, 1.0);
    let cave_dim = mix(1.0, 0.62, underground);
    let ambient_scalar = mix(0.16, 0.88, day) * cave_dim;
    let sky_fill = mix(0.04, 0.20, day)
        * (0.45 + 0.55 * clamp(normal.y * 0.5 + 0.5, 0.0, 1.0))
        * mix(1.0, 0.74, underground);
    let sun_scalar = (0.24 + 0.60 * pow(ndl, 1.45)) * day * shadow;
    let moon_scalar = (0.03 + 0.14 * pow(nml, 1.25)) * night * 0.72;
    let light_scalar = clamp(face_light * (ambient_scalar + sun_scalar + moon_scalar) + sky_fill, 0.03, 1.55);

    let tint = mix(vec3<f32>(0.78, 0.85, 1.06), vec3<f32>(1.0, 1.0, 1.0), day);
    let lit_linear = max(base * light_scalar * tint * u.settings.w, vec3<f32>(0.0));
    let lit = pow(tonemap_reinhard(lit_linear), vec3<f32>(1.0 / 2.1));
    var shaded = lit;

    let rain = max(clamp(u.weather.x, 0.0, 1.0), clamp(u.weather.z, 0.0, 1.0) * 0.82);
    if rain > 0.02 {
        let top_flat = smoothstep(0.94, 0.999, normal.y);
        let open_factor = select(0.0, 1.0, voxel_id(cell + vec3<i32>(0, 1, 0)) == 0u);
        let candidate = select(0.0, 1.0, is_puddle_candidate(hit_id));
        let animated_xz = hit_world.xz + vec2<f32>(u.weather.y * 0.015, -u.weather.y * 0.011);
        let puddle = puddle_pattern(animated_xz, rain) * top_flat * open_factor * candidate;

        if puddle > 0.001 {
            let ripple_a = hash12(floor(hit_world.xz * 9.0 + vec2<f32>(u.weather.y * 2.2, 13.0))) - 0.5;
            let ripple_b = hash12(floor(hit_world.zx * 9.0 + vec2<f32>(-17.0, u.weather.y * 2.2))) - 0.5;
            let wet_normal = normalize(normal + vec3<f32>(ripple_a, 0.0, ripple_b) * (0.09 * puddle));
            let refl_dir = normalize(reflect(rd_world, wet_normal));
            let reflection = trace_reflection_color(hit_world + wet_normal * 0.04, refl_dir, day_time);

            let view_ndot = clamp(dot(-rd_world, wet_normal), 0.0, 1.0);
            let fresnel = pow(1.0 - view_ndot, 5.0);
            let reflectivity = puddle * mix(0.20, 0.70, fresnel);
            let sun_spec = pow(max(dot(reflect(-sun_dir, wet_normal), -rd_world), 0.0), 72.0)
                * day
                * puddle
                * 0.22;

            let wet_diffuse = mix(lit, lit * 0.78, puddle * 0.38);
            shaded = mix(wet_diffuse, reflection, reflectivity) + vec3<f32>(sun_spec);
        }
    }

    let dist = t;
    let fog_density = mix(0.0022, 0.00105, day);
    let fog = clamp(1.0 - exp(-dist * fog_density), 0.0, 1.0);
    let sky = sky_color(day_time, rd_world.y);
    let color = mix(shaded, sky, fog * 0.88);

    return vec4<f32>(color, 1.0);
}

fn sky_color(time: f32, ray_y: f32) -> vec3<f32> {
    let angle = time * 6.28318530718;
    let sun_y = sin(angle);
    let day = smoothstep(-0.08, 0.14, sun_y);
    let twilight = 1.0 - smoothstep(0.02, 0.42, abs(sun_y));

    var sky = mix(vec3<f32>(0.015, 0.025, 0.060), vec3<f32>(0.52, 0.77, 0.97), day);
    sky = mix(sky, vec3<f32>(1.00, 0.54, 0.23), twilight * 0.55);

    let horizon = smoothstep(-0.35, 0.22, ray_y);
    return mix(sky * 0.78, sky, horizon);
}

fn smoothstep(edge0: f32, edge1: f32, x: f32) -> f32 {
    let t = clamp((x - edge0) / (edge1 - edge0), 0.0, 1.0);
    return t * t * (3.0 - 2.0 * t);
}
