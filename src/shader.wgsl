struct Camera {
    view_proj: mat4x4<f32>,
}
@group(0) @binding(0)
var<uniform> cam: Camera;

struct VertIn {
    @location(0) pos:     vec3<f32>,
    @location(1) normal:  vec3<f32>,
    @location(2) tex_idx: u32,
}

struct VertOut {
    @builtin(position) clip_pos: vec4<f32>,
    @location(0) normal:   vec3<f32>,
    @location(1) tex_idx:  u32,
    @location(2) world_y:  f32,
}

@vertex
fn vs_main(v: VertIn) -> VertOut {
    var out: VertOut;
    out.clip_pos = cam.view_proj * vec4<f32>(v.pos, 1.0);
    out.normal   = v.normal;
    out.tex_idx  = v.tex_idx;
    out.world_y  = v.pos.y;
    return out;
}

@fragment
fn fs_main(in: VertOut) -> @location(0) vec4<f32> {
    // Освещение
    let sun     = normalize(vec3<f32>(0.6, 1.0, 0.4));
    let diffuse = max(dot(in.normal, sun), 0.0);
    // Боковые грани чуть темнее — имитация AO
    let side_dim = select(1.0, 0.85, abs(in.normal.x) + abs(in.normal.z) > 0.5);
    let ambient  = 0.35;
    let light    = (ambient + diffuse * 0.65) * side_dim;

    var color: vec3<f32>;
    switch in.tex_idx {
        case 1u: {
            // Трава: верхняя грань зелёная, боковые — земля
            if in.normal.y > 0.5 {
                color = vec3(0.28, 0.58, 0.18);
            } else {
                color = vec3(0.48, 0.33, 0.18);
            }
        }
        case 2u: { color = vec3(0.48, 0.33, 0.18); } // Dirt
        case 3u: {
            // Камень — небольшая вариация по высоте
            let v = 0.45 + fract(in.world_y * 0.1) * 0.08;
            color = vec3(v, v, v);
        }
        case 4u: { color = vec3(0.85, 0.80, 0.50); } // Sand
        case 5u: {
            // Вода — полупрозрачный синий
            color = vec3(0.18, 0.42, 0.78);
        }
        case 6u: { color = vec3(0.15, 0.12, 0.12); } // Bedrock
        default: { color = vec3(1.0, 0.0, 1.0); }
    }

    // Туман вдаль
    let fog_start = 180.0;
    let fog_end   = 320.0;
    let dist      = in.clip_pos.w;
    let fog       = clamp((dist - fog_start) / (fog_end - fog_start), 0.0, 1.0);
    let sky_color = vec3(0.53, 0.81, 0.98);

    let final_color = mix(color * light, sky_color, fog);
    return vec4<f32>(final_color, 1.0);
}