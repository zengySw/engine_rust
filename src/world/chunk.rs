use noise::{Fbm, NoiseFn, Perlin, SuperSimplex};
use bytemuck::{Pod, Zeroable};
use super::block::Block;
use super::biome::{self, Biome};
use std::sync::Arc;

pub const CHUNK_W: usize = 16;
pub const CHUNK_H: usize = 256;
pub const CHUNK_D: usize = 16;
pub const SEA_LEVEL: usize = 63;

#[inline(always)]
pub fn idx(x: usize, y: usize, z: usize) -> usize {
    x * CHUNK_H * CHUNK_D + y * CHUNK_D + z
}

// ─── Heightmap ────────────────────────────────────────────────────────────────

#[derive(Clone)]
pub struct Heightmap {
    pub surface: Box<[[u32; CHUNK_D]; CHUNK_W]>,
    pub biome:   Box<[[Biome; CHUNK_D]; CHUNK_W]>,
}

// ─── TerrainSampler ──────────────────────────────────────────────────────────

struct TerrainSampler {
    continental: Perlin,
    erosion: Fbm<Perlin>,
    peaks: Fbm<Perlin>,
    detail: Perlin,
    warp_x: Perlin,
    warp_z: Perlin,
    temperature: SuperSimplex,
    moisture: SuperSimplex,
    weirdness: SuperSimplex,
}
#[derive(Clone, Copy)]
struct SampleFields {
    surface: u32,
    height_f: f64,
    temperature: f64,
    humidity: f64,
    continentalness: f64,
    erosion: f64,
    weirdness: f64,
}
impl TerrainSampler {
    fn new(seed: u32) -> Self {
        let continental = Perlin::new(seed.wrapping_add(10));
        let detail = Perlin::new(seed.wrapping_add(11));
        let warp_x = Perlin::new(seed.wrapping_add(17));
        let warp_z = Perlin::new(seed.wrapping_add(18));
        let mut erosion: Fbm<Perlin> = Fbm::new(seed.wrapping_add(12));
        erosion.octaves = 4;
        erosion.frequency = 1.0;
        erosion.lacunarity = 2.0;
        erosion.persistence = 0.5;
        let mut peaks: Fbm<Perlin> = Fbm::new(seed.wrapping_add(13));
        peaks.octaves = 5;
        peaks.frequency = 1.0;
        peaks.lacunarity = 2.0;
        peaks.persistence = 0.5;
        let temperature = SuperSimplex::new(seed.wrapping_add(14));
        let moisture = SuperSimplex::new(seed.wrapping_add(15));
        let weirdness = SuperSimplex::new(seed.wrapping_add(16));
        Self {
            continental,
            erosion,
            peaks,
            detail,
            warp_x,
            warp_z,
            temperature,
            moisture,
            weirdness,
        }
    }
    fn sample_fields(&self, wx: f64, wz: f64) -> SampleFields {
        // Domain warp: smoother macro regions and natural curved biome borders.
        let macro_warp_x = self.warp_x.get([wx / 1400.0, wz / 1400.0]) * 92.0;
        let macro_warp_z = self.warp_z.get([wx / 1400.0 + 11.3, wz / 1400.0 - 7.9]) * 92.0;
        let terrain_x = wx + macro_warp_x * 0.36;
        let terrain_z = wz + macro_warp_z * 0.36;
        let climate_x = wx + macro_warp_x;
        let climate_z = wz + macro_warp_z;
        let micro_warp_x = self.warp_x.get([wx / 220.0 - 21.7, wz / 220.0 + 13.4]) * 16.0;
        let micro_warp_z = self.warp_z.get([wx / 220.0 + 31.1, wz / 220.0 - 17.8]) * 16.0;
        let climate_x = climate_x + micro_warp_x;
        let climate_z = climate_z + micro_warp_z;
        let continental_raw = self.continental.get([terrain_x / 980.0, terrain_z / 980.0]);
        let erosion_raw = self.erosion.get([terrain_x / 450.0, terrain_z / 450.0]);
        let peaks_raw = self.peaks.get([terrain_x / 280.0, terrain_z / 280.0]);
        let detail = self.detail.get([terrain_x / 140.0, terrain_z / 140.0]);
        let continental = remap01(continental_raw);
        let erosion_n = remap01(erosion_raw);
        let land = smoothstep(remap01((continental - 0.15) * 1.2));
        let ridged = (1.0 - (peaks_raw * 1.6).abs()).clamp(0.0, 1.0).powf(1.8);
        let mountain_mask = smoothstep(((land - 0.50) / 0.45).clamp(0.0, 1.0));
        // Softer macro relief to avoid giant staircase-like walls.
        let base = SEA_LEVEL as f64 - 14.0 + land * 42.0;
        let hills = detail * (4.0 + (1.0 - erosion_n) * 6.0);
        let mountain_raw = ridged * mountain_mask;
        let mountain = mountain_raw.powf(1.15) * (20.0 + (1.0 - erosion_n) * 22.0);
        let plateau_soften = smoothstep(((land - 0.64) / 0.24).clamp(0.0, 1.0));
        let height_f = (base + hills + mountain * (1.0 - 0.18 * plateau_soften))
            .clamp(4.0, (CHUNK_H - 2) as f64);
        let surface = height_f.round() as u32;
        let mut temperature = remap01(self.temperature.get([climate_x / 720.0, climate_z / 720.0]));
        // Very-low-frequency latitude term to create broad climate belts.
        let lat_bias = (climate_z / 5200.0).sin() * 0.12;
        temperature = (temperature + lat_bias).clamp(0.0, 1.0);
        temperature -= ((height_f - SEA_LEVEL as f64).max(0.0)) * 0.0026;
        let temperature = (temperature * 2.0 - 1.0).clamp(-1.0, 1.0);
        let mut moisture = remap01(self.moisture.get([climate_x / 660.0, climate_z / 660.0]));
        let weirdness = self.weirdness.get([climate_x / 480.0, climate_z / 480.0]);
        // Valley corridors increase erosion locally and produce natural river chains.
        let valley = self.detail.get([terrain_x / 205.0 + 17.0, terrain_z / 205.0 - 9.0]).abs();
        let valley_push = ((0.20 - valley) / 0.20).clamp(0.0, 1.0) * 0.28;
        let erosion = (erosion_raw + valley_push).clamp(-1.0, 1.0);
        // Moisture advection: coasts/valleys are wetter, high inland ridges are drier.
        let coastal_wet = ((0.18 - continental_raw) / 0.55).clamp(0.0, 1.0) * 0.16;
        let inland_dry = ((continental_raw - 0.22) / 0.78).clamp(0.0, 1.0) * 0.18;
        let valley_wet = ((0.23 - valley) / 0.23).clamp(0.0, 1.0) * 0.12;
        let alpine_dry = ((height_f - (SEA_LEVEL as f64 + 40.0)) / 110.0).clamp(0.0, 1.0) * 0.09;
        moisture = (moisture + coastal_wet + valley_wet - inland_dry - alpine_dry).clamp(0.0, 1.0);
        let humidity = (moisture * 2.0 - 1.0).clamp(-1.0, 1.0);
        SampleFields {
            surface,
            height_f,
            temperature,
            humidity,
            continentalness: continental_raw.clamp(-1.0, 1.0),
            erosion,
            weirdness,
        }
    }
    fn sample_surface_and_biome(&self, wx: f64, wz: f64) -> (u32, Biome) {
        let center = self.sample_fields(wx, wz);
        let center_biome = biome::select_biome(
            center.temperature,
            center.humidity,
            center.continentalness,
            center.erosion,
            center.weirdness,
        );
        // Boundary anti-aliasing: only near climate borders, blend with nearby samples.
        let boundary = biome_boundary_factor(
            center.temperature,
            center.humidity,
            center.continentalness,
            center.erosion,
        );
        if boundary < 0.38 {
            return (center.surface, center_biome);
        }
        let mut scores: Vec<(Biome, f32)> = Vec::with_capacity(10);
        add_biome_score(&mut scores, center_biome, 1.55);
        let radius = 16.0 + boundary * 18.0;
        let diag = radius * 0.72;
        let offsets = [
            (radius, 0.0, 0.98),
            (-radius, 0.0, 0.98),
            (0.0, radius, 0.98),
            (0.0, -radius, 0.98),
            (diag, diag, 0.74),
            (-diag, diag, 0.74),
            (diag, -diag, 0.74),
            (-diag, -diag, 0.74),
        ];
        for (dx, dz, dir_w) in offsets {
            let s = self.sample_fields(wx + dx, wz + dz);
            let b = biome::select_biome(
                s.temperature,
                s.humidity,
                s.continentalness,
                s.erosion,
                s.weirdness,
            );
            let height_w = (1.0 - ((s.height_f - center.height_f).abs() / 28.0)).clamp(0.22, 1.0);
            let climate_w = (1.0
                - (s.temperature - center.temperature).abs() * 0.60
                - (s.humidity - center.humidity).abs() * 0.55
                - (s.continentalness - center.continentalness).abs() * 0.45
                - (s.erosion - center.erosion).abs() * 0.35)
                .clamp(0.20, 1.0);
            let base_w = dir_w;
            add_biome_score(&mut scores, b, (base_w * height_w * boundary) as f32);
            add_biome_score(
                &mut scores,
                b,
                (base_w * climate_w * boundary * 0.58) as f32,
            );
        }
        let mut best_biome = center_biome;
        let mut best_score = f32::MIN;
        let mut center_score = 0.0f32;
        for (b, s) in &scores {
            if *b == center_biome {
                center_score = *s;
            }
            if *s > best_score {
                best_score = *s;
                best_biome = *b;
            }
        }
        let selected = if best_biome != center_biome && best_score > center_score * 1.10 {
            best_biome
        } else {
            center_biome
        };
        (center.surface, selected)
    }
}

impl Heightmap {
    #[allow(dead_code)]
    pub fn generate(cx: i32, cz: i32, seed: u32) -> Self {
        let sampler = TerrainSampler::new(seed);
        let world_x0 = cx * CHUNK_W as i32;
        let world_z0 = cz * CHUNK_D as i32;

        let mut surface = Box::new([[0u32; CHUNK_D]; CHUNK_W]);
        let mut biome_map = Box::new([[Biome::Plains; CHUNK_D]; CHUNK_W]);

        for x in 0..CHUNK_W {
            let wx = (world_x0 + x as i32) as f64;
            for z in 0..CHUNK_D {
                let wz = (world_z0 + z as i32) as f64;

                let (h, b) = sampler.sample_surface_and_biome(wx, wz);
                surface[x][z]   = h;
                biome_map[x][z] = b;
            }
        }

        Self { surface, biome: biome_map }
    }

    #[allow(dead_code)]
    pub fn blended_surface(
        &self,
        x: usize,
        z: usize,
        neighbors: [Option<&Heightmap>; 4],
    ) -> u32 {
        let base    = self.surface[x][z] as f32;
        let mut sum = base;
        let mut w   = 1.0f32;

        if x >= CHUNK_W - 2 {
            if let Some(nb) = neighbors[1] {
                let t = (x as f32 - (CHUNK_W - 2) as f32) / 2.0;
                sum += nb.surface[0][z] as f32 * t;
                w   += t;
            }
        }
        if x <= 1 {
            if let Some(nb) = neighbors[0] {
                let t = 1.0 - x as f32 / 2.0;
                sum += nb.surface[CHUNK_W - 1][z] as f32 * t;
                w   += t;
            }
        }
        if z >= CHUNK_D - 2 {
            if let Some(nb) = neighbors[3] {
                let t = (z as f32 - (CHUNK_D - 2) as f32) / 2.0;
                sum += nb.surface[x][0] as f32 * t;
                w   += t;
            }
        }
        if z <= 1 {
            if let Some(nb) = neighbors[2] {
                let t = 1.0 - z as f32 / 2.0;
                sum += nb.surface[x][CHUNK_D - 1] as f32 * t;
                w   += t;
            }
        }

        ((sum / w) as u32).clamp(4, CHUNK_H as u32 - 2)
    }

    #[allow(dead_code)]
    pub fn blended_biome(
        &self,
        x: usize,
        z: usize,
        neighbors: [Option<&Heightmap>; 4],
    ) -> Biome {
        let center = self.biome[x][z];

        let left = if x > 0 {
            self.biome[x - 1][z]
        } else {
            neighbors[0].map_or(center, |nb| nb.biome[CHUNK_W - 1][z])
        };
        let right = if x + 1 < CHUNK_W {
            self.biome[x + 1][z]
        } else {
            neighbors[1].map_or(center, |nb| nb.biome[0][z])
        };
        let down = if z > 0 {
            self.biome[x][z - 1]
        } else {
            neighbors[2].map_or(center, |nb| nb.biome[x][CHUNK_D - 1])
        };
        let up = if z + 1 < CHUNK_D {
            self.biome[x][z + 1]
        } else {
            neighbors[3].map_or(center, |nb| nb.biome[x][0])
        };
        let around = [left, right, down, up];

        let mut same_as_center = 0usize;
        for b in around {
            if b == center {
                same_as_center += 1;
            }
        }
        if same_as_center >= 2 {
            return center;
        }

        let mut best = center;
        let mut best_count = 0usize;
        for i in 0..around.len() {
            let b = around[i];
            let mut count = 1usize;
            for &other in around.iter().skip(i + 1) {
                if other == b {
                    count += 1;
                }
            }
            if count > best_count {
                best = b;
                best_count = count;
            }
        }

        if best_count >= 2 { best } else { center }
    }
}

#[inline]
fn remap01(v: f64) -> f64 {
    ((v + 1.0) * 0.5).clamp(0.0, 1.0)
}

#[inline]
fn smoothstep(t: f64) -> f64 {
    let tt = t.clamp(0.0, 1.0);
    tt * tt * (3.0 - 2.0 * tt)
}

#[inline]
fn threshold_edge(v: f64, thresholds: &[f64], width: f64) -> f64 {
    let mut edge = 0.0f64;
    let half = width * 0.5;
    for &t in thresholds {
        let d = ((v - t).abs() / half).clamp(0.0, 1.0);
        // d=0 near threshold (strong boundary), d=1 far away.
        let near = 1.0 - smoothstep(d);
        edge = edge.max(near);
    }
    edge
}

#[inline]
fn biome_boundary_factor(temperature: f64, humidity: f64, continentalness: f64, erosion: f64) -> f64 {
    const TEMP_T: [f64; 4] = [-0.45, -0.15, 0.20, 0.55];
    const HUM_T: [f64; 4] = [-0.35, -0.10, 0.10, 0.30];
    const CONT_T: [f64; 5] = [-0.45, -0.19, 0.03, 0.30, 0.55];
    const ERO_T: [f64; 6] = [-0.78, -0.375, -0.2225, 0.05, 0.45, 0.55];
    let t = threshold_edge(temperature, &TEMP_T, 0.11);
    let h = threshold_edge(humidity, &HUM_T, 0.11);
    let c = threshold_edge(continentalness, &CONT_T, 0.14);
    let e = threshold_edge(erosion, &ERO_T, 0.10);
    (0.24 * t + 0.24 * h + 0.30 * c + 0.22 * e).clamp(0.0, 1.0)
}

#[inline]
fn add_biome_score(scores: &mut Vec<(Biome, f32)>, biome: Biome, weight: f32) {
    if let Some((_, score)) = scores.iter_mut().find(|(b, _)| *b == biome) {
        *score += weight;
    } else {
        scores.push((biome, weight));
    }
}

pub fn spawn_point(seed: u32) -> (f32, f32, f32) {
    let sampler = TerrainSampler::new(seed);

    let mut best: Option<(i32, i32, u32, i32)> = None;

    for radius in (0i32..=256).step_by(8) {
        for wx in -radius..=radius {
            for wz in -radius..=radius {
                if wx.abs() != radius && wz.abs() != radius {
                    continue;
                }

                let (surface, biome) = sampler.sample_surface_and_biome(wx as f64, wz as f64);
                if biome::is_ocean(biome) || surface <= SEA_LEVEL as u32 {
                    continue;
                }

                let slope = spawn_slope(&sampler, wx, wz, surface as i32);
                if slope > 6 {
                    continue;
                }

                let score = radius * 10 + slope;
                match best {
                    Some((_, _, _, best_score)) if score >= best_score => {}
                    _ => best = Some((wx, wz, surface, score)),
                }
            }
        }

        if best.is_some() && radius >= 32 {
            break;
        }
    }

    let (sx, sz, sy) = if let Some((x, z, y, _)) = best {
        (x as f32 + 0.5, z as f32 + 0.5, y as f32 + 1.05)
    } else {
        let (y, _) = sampler.sample_surface_and_biome(0.0, 0.0);
        (0.5, 0.5, y as f32 + 1.05)
    };

    (sx, sy, sz)
}

fn spawn_slope(sampler: &TerrainSampler, wx: i32, wz: i32, center_h: i32) -> i32 {
    let samples = [
        sampler.sample_surface_and_biome((wx + 1) as f64, wz as f64).0 as i32,
        sampler.sample_surface_and_biome((wx - 1) as f64, wz as f64).0 as i32,
        sampler.sample_surface_and_biome(wx as f64, (wz + 1) as f64).0 as i32,
        sampler.sample_surface_and_biome(wx as f64, (wz - 1) as f64).0 as i32,
    ];

    samples
        .into_iter()
        .map(|h| (h - center_h).abs())
        .max()
        .unwrap_or(0)
}

// ─── Chunk ───────────────────────────────────────────────────────────────────

#[derive(Clone)]
pub struct Chunk {
    pub cx:        i32,
    pub cz:        i32,
    pub blocks:    Arc<[Block]>,
    pub heightmap: Arc<Heightmap>,
}

impl Chunk {
    pub fn generate(cx: i32, cz: i32, seed: u32) -> Self {
        let sampler = TerrainSampler::new(seed);
        let cave_a = Perlin::new(seed.wrapping_add(20));
        let cave_b = Perlin::new(seed.wrapping_add(21));
        let size = CHUNK_W * CHUNK_H * CHUNK_D;
        let mut blocks = vec![Block::Air; size].into_boxed_slice();

        let mut hmap = Heightmap {
            surface: Box::new([[0u32; CHUNK_D]; CHUNK_W]),
            biome: Box::new([[Biome::Plains; CHUNK_D]; CHUNK_W]),
        };
        for x in 0..CHUNK_W {
            for z in 0..CHUNK_D {
                let wx = (cx * CHUNK_W as i32 + x as i32) as f64;
                let wz = (cz * CHUNK_D as i32 + z as i32) as f64;
                let (surface, biome) = sampler.sample_surface_and_biome(wx, wz);
                hmap.surface[x][z] = surface;
                hmap.biome[x][z] = biome;
            }
        }

        // chunk + one-cell border sampled from world-space noise
        let mut surface_ctx = [[0u32; CHUNK_D + 2]; CHUNK_W + 2];
        let mut biome_ctx = [[Biome::Plains; CHUNK_D + 2]; CHUNK_W + 2];
        for x in 0..CHUNK_W {
            for z in 0..CHUNK_D {
                surface_ctx[x + 1][z + 1] = hmap.surface[x][z];
                biome_ctx[x + 1][z + 1] = hmap.biome[x][z];
            }
        }
        for ex in 0..(CHUNK_W + 2) {
            for ez in 0..(CHUNK_D + 2) {
                if ex > 0 && ex < CHUNK_W + 1 && ez > 0 && ez < CHUNK_D + 1 {
                    continue;
                }
                let wx = (cx * CHUNK_W as i32 + ex as i32 - 1) as f64;
                let wz = (cz * CHUNK_D as i32 + ez as i32 - 1) as f64;
                let (surface, biome) = sampler.sample_surface_and_biome(wx, wz);
                surface_ctx[ex][ez] = surface;
                biome_ctx[ex][ez] = biome;
            }
        }
        // Relief smoothing pass to reduce harsh cliff terraces while keeping macro shape.
        let mut smoothed = [[0u32; CHUNK_D]; CHUNK_W];
        for x in 0..CHUNK_W {
            for z in 0..CHUNK_D {
                let ex = x + 1;
                let ez = z + 1;
                let center = surface_ctx[ex][ez] as f32;
                let card_avg = (surface_ctx[ex - 1][ez] as f32
                    + surface_ctx[ex + 1][ez] as f32
                    + surface_ctx[ex][ez - 1] as f32
                    + surface_ctx[ex][ez + 1] as f32)
                    * 0.25;
                let diag_avg = (surface_ctx[ex - 1][ez - 1] as f32
                    + surface_ctx[ex + 1][ez - 1] as f32
                    + surface_ctx[ex - 1][ez + 1] as f32
                    + surface_ctx[ex + 1][ez + 1] as f32)
                    * 0.25;
                let local_avg = card_avg * 0.70 + diag_avg * 0.30;
                let slope = ((surface_ctx[ex + 1][ez] as i32 - surface_ctx[ex - 1][ez] as i32).abs())
                    .max((surface_ctx[ex][ez + 1] as i32 - surface_ctx[ex][ez - 1] as i32).abs())
                    as f32;
                let mut local_min = surface_ctx[ex][ez];
                let mut local_max = surface_ctx[ex][ez];
                for sx in (ex - 1)..=(ex + 1) {
                    for sz in (ez - 1)..=(ez + 1) {
                        local_min = local_min.min(surface_ctx[sx][sz]);
                        local_max = local_max.max(surface_ctx[sx][sz]);
                    }
                }
                let relief = (local_max - local_min) as f32;
                let steep = (slope - 4.0).max(0.0);
                let relief_term = (relief - 9.0).max(0.0) * 0.25;
                let mut blend = ((steep + relief_term) / 8.0).clamp(0.0, 1.0);
                let alpine_preserve = ((center - (SEA_LEVEL as f32 + 54.0)).max(0.0) / 72.0)
                    .clamp(0.0, 0.35);
                blend = (blend - alpine_preserve).clamp(0.0, 1.0);

                let mut shaped = center * (1.0 - blend) + local_avg * blend;
                let hard_cap = local_avg + 7.0;
                if shaped > hard_cap {
                    shaped = hard_cap;
                }
                smoothed[x][z] = shaped
                    .round()
                    .clamp(4.0, (CHUNK_H - 2) as f32) as u32;
            }
        }
        for x in 0..CHUNK_W {
            for z in 0..CHUNK_D {
                hmap.surface[x][z] = smoothed[x][z];
                surface_ctx[x + 1][z + 1] = smoothed[x][z];
            }
        }

        for x in 0..CHUNK_W {
            for z in 0..CHUNK_D {
                let ex = x + 1;
                let ez = z + 1;
                let b = biome_ctx[ex][ez];
                let surface_u = surface_ctx[ex][ez];
                let surface = surface_u as usize;
                let slope = ((surface_ctx[ex + 1][ez] as i32 - surface_ctx[ex - 1][ez] as i32).abs())
                    .max((surface_ctx[ex][ez + 1] as i32 - surface_ctx[ex][ez - 1] as i32).abs());
                let wx = (cx * CHUNK_W as i32 + x as i32) as f64;
                let wz = (cz * CHUNK_D as i32 + z as i32) as f64;
                let wx_i = wx as i32;
                let wz_i = wz as i32;
                let hseed = hash2(seed, wx as i32, wz as i32);
                let around_biomes = [
                    biome_ctx[ex - 1][ez],
                    biome_ctx[ex + 1][ez],
                    biome_ctx[ex][ez - 1],
                    biome_ctx[ex][ez + 1],
                    biome_ctx[ex - 1][ez - 1],
                    biome_ctx[ex + 1][ez - 1],
                    biome_ctx[ex - 1][ez + 1],
                    biome_ctx[ex + 1][ez + 1],
                ];
                let around_heights = [
                    surface_ctx[ex - 1][ez],
                    surface_ctx[ex + 1][ez],
                    surface_ctx[ex][ez - 1],
                    surface_ctx[ex][ez + 1],
                    surface_ctx[ex - 1][ez - 1],
                    surface_ctx[ex + 1][ez - 1],
                    surface_ctx[ex - 1][ez + 1],
                    surface_ctx[ex + 1][ez + 1],
                ];
                let profile = biome::blended_surface_profile_weighted(
                    b,
                    around_biomes,
                    surface_u,
                    around_heights,
                    slope,
                    SEA_LEVEL as u32,
                    hseed,
                );
                let mut top_block = profile.top;
                let mut filler_block = profile.filler;
                let mut fill_depth = profile.depth;
                // Hard cap only for truly extreme cliffs/high alpine peaks.
                let cliff_rock = surface > SEA_LEVEL + 18 && slope >= 9;
                let alpine_rock = surface > 190;
                if cliff_rock || alpine_rock {
                    top_block = Block::Stone;
                    filler_block = Block::Stone;
                    fill_depth = fill_depth.max(if alpine_rock { 6 } else { 5 });
                }
                blocks[idx(x, 0, z)] = Block::Bedrock;
                let solid_max = surface.saturating_sub(fill_depth);
                let cave_thr = biome::cave_threshold(b);
                let cave_limit = surface.saturating_sub(8);
                if solid_max > 1 {
                    for y in 1..solid_max.min(CHUNK_H) {
                        let can_cave = y > 8 && y < cave_limit;
                        if can_cave {
                            let n0 = cave_a.get([wx / 26.0, y as f64 / 22.0, wz / 26.0]);
                            let n1 = cave_b.get([wx / 14.0, y as f64 / 14.0, wz / 14.0]).abs();
                            if !(n0 > cave_thr && n1 > 0.33) {
                                blocks[idx(x, y, z)] = pick_stone_block(seed, wx_i, y as i32, wz_i, b);
                            } else {
                                blocks[idx(x, y, z)] = Block::CaveAir;
                            }
                        } else {
                            blocks[idx(x, y, z)] = pick_stone_block(seed, wx_i, y as i32, wz_i, b);
                        }
                    }
                }
                let fill_start = solid_max.max(1);
                if fill_start < surface {
                    for y in fill_start..surface.min(CHUNK_H) {
                        blocks[idx(x, y, z)] = filler_block;
                    }
                }
                if surface < CHUNK_H {
                    blocks[idx(x, surface, z)] = top_block;
                }
                if surface < SEA_LEVEL {
                    let water_top = SEA_LEVEL.min(CHUNK_H - 1);
                    for y in (surface + 1)..=water_top {
                        blocks[idx(x, y, z)] = Block::Water;
                    }
                }
            }
        }
        place_trees(&mut blocks, cx, cz, seed, &hmap);
        place_boulders(&mut blocks, cx, cz, seed, &hmap);
        Self {
            cx,
            cz,
            blocks: blocks.into(),
            heightmap: Arc::new(hmap),
        }
    }

    #[inline]
    pub fn get(&self, x: usize, y: usize, z: usize) -> Block {
        self.blocks[idx(x, y, z)]
    }

    pub fn build_mesh(
        &self,
        neighbor_px: Option<&Chunk>,
        neighbor_nx: Option<&Chunk>,
        neighbor_pz: Option<&Chunk>,
        neighbor_nz: Option<&Chunk>,
    ) -> Vec<Vertex> {
        let mut verts = Vec::with_capacity(4096);

        const FACES: [([f32; 3], [[f32; 3]; 4]); 6] = [
            ([1., 0., 0.],  [[1.,0.,0.],[1.,1.,0.],[1.,1.,1.],[1.,0.,1.]]),
            ([-1.,0., 0.],  [[0.,0.,1.],[0.,1.,1.],[0.,1.,0.],[0.,0.,0.]]),
            ([0., 1., 0.],  [[0.,1.,0.],[0.,1.,1.],[1.,1.,1.],[1.,1.,0.]]),
            ([0.,-1., 0.],  [[0.,0.,1.],[0.,0.,0.],[1.,0.,0.],[1.,0.,1.]]),
            ([0., 0., 1.],  [[1.,0.,1.],[1.,1.,1.],[0.,1.,1.],[0.,0.,1.]]),
            ([0., 0.,-1.],  [[0.,0.,0.],[0.,1.,0.],[1.,1.,0.],[1.,0.,0.]]),
        ];

        let ox = (self.cx * CHUNK_W as i32) as f32;
        let oz = (self.cz * CHUNK_D as i32) as f32;

        for x in 0..CHUNK_W {
            for z in 0..CHUNK_D {
                let surface_y = self.heightmap.surface[x][z] as usize;
                let mut col_top = (surface_y + 1).max(SEA_LEVEL + 1).min(CHUNK_H);

                let scan_max = (surface_y + 10).min(CHUNK_H - 1);
                if scan_max > surface_y {
                    for y in (surface_y + 1..=scan_max).rev() {
                        if !self.get(x, y, z).is_air() {
                            col_top = (y + 1).min(CHUNK_H);
                            break;
                        }
                    }
                }

                for y in 0..col_top {
                    let block = self.get(x, y, z);
                    if !block.is_solid() { continue; }

                    let solid = |bx: i32, by: i32, bz: i32| -> bool {
                        if by < 0 || by >= CHUNK_H as i32 { return by < 0; }
                        if bx < 0 {
                            return neighbor_px.map_or(false, |c| {
                                c.get(CHUNK_W - 1, by as usize, bz as usize).is_solid()
                            });
                        }
                        if bx >= CHUNK_W as i32 {
                            return neighbor_nx.map_or(false, |c| c.get(0, by as usize, bz as usize).is_solid());
                        }
                        if bz < 0 {
                            return neighbor_pz.map_or(false, |c| {
                                c.get(bx as usize, by as usize, CHUNK_D - 1).is_solid()
                            });
                        }
                        if bz >= CHUNK_D as i32 {
                            return neighbor_nz.map_or(false, |c| c.get(bx as usize, by as usize, 0).is_solid());
                        }
                        self.get(bx as usize, by as usize, bz as usize).is_solid()
                    };

                    let xi = x as i32;
                    let yi = y as i32;
                    let zi = z as i32;
                    let neighbors = [
                        solid(xi+1,yi,zi), solid(xi-1,yi,zi),
                        solid(xi,yi+1,zi), solid(xi,yi-1,zi),
                        solid(xi,yi,zi+1), solid(xi,yi,zi-1),
                    ];

                    for (i, (normal, corners)) in FACES.iter().enumerate() {
                        if neighbors[i] { continue; }
                        let tex  = face_texture_for(block, i);
                        let uvs  = [[0.0f32,1.0],[0.0,0.0],[1.0,0.0],[1.0,1.0]];
                        let v: [Vertex; 4] = std::array::from_fn(|j| Vertex {
                            pos: [
                                ox + x as f32 + corners[j][0],
                                y as f32 + corners[j][1],
                                oz + z as f32 + corners[j][2],
                            ],
                            normal: *normal,
                            tex_idx: tex,
                            uv: uvs[j],
                        });
                        verts.extend_from_slice(&[v[0],v[1],v[2],v[0],v[2],v[3]]);
                    }
                }
            }
        }

        verts
    }
}

#[inline]
fn face_texture_for(block: Block, face_idx: usize) -> u32 {
    // Keep in sync with renderer texture array order:
    // grass_top idx=15, workbench_top idx=19, workbench_front idx=20,
    // furnace idx=21, furnace_top idx=22, furnace_front idx=23.
    const GRASS_TOP_TEX_IDX: u32 = 15;
    const WORKBENCH_TOP_TEX_IDX: u32 = 19;
    const WORKBENCH_FRONT_TEX_IDX: u32 = 20;
    const FURNACE_TOP_TEX_IDX: u32 = 22;
    const FURNACE_FRONT_TEX_IDX: u32 = 23;

    match block {
        Block::Grass => {
            if face_idx == 2 {
                GRASS_TOP_TEX_IDX
            } else if face_idx == 3 {
                Block::Dirt.texture_index()
            } else {
                Block::Grass.texture_index()
            }
        }
        Block::Log => {
            if face_idx == 2 || face_idx == 3 { Block::LogBottom.texture_index() }
            else                               { Block::Log.texture_index()       }
        }
        Block::Workbench => {
            if face_idx == 2 {
                WORKBENCH_TOP_TEX_IDX
            } else if face_idx == 4 {
                WORKBENCH_FRONT_TEX_IDX
            } else {
                Block::Workbench.texture_index()
            }
        }
        Block::Furnace => {
            if face_idx == 2 || face_idx == 3 {
                FURNACE_TOP_TEX_IDX
            } else if face_idx == 4 {
                FURNACE_FRONT_TEX_IDX
            } else {
                Block::Furnace.texture_index()
            }
        }
        _ => block.texture_index(),
    }
}

// ─── Посадка деревьев ─────────────────────────────────────────────────────────

fn place_trees(blocks: &mut [Block], cx: i32, cz: i32, seed: u32, hmap: &Heightmap) {
    let world_x0 = cx * CHUNK_W as i32;
    let world_z0 = cz * CHUNK_D as i32;
    // We only need one-cell margin for neighborhood sampling;
    // actual crown fit is validated per-tree below.
    let sample_margin = 1usize;
    for x in sample_margin..(CHUNK_W - sample_margin) {
        let wx = world_x0 + x as i32;
        for z in sample_margin..(CHUNK_D - sample_margin) {
            let wz = world_z0 + z as i32;
            let b = hmap.biome[x][z];
            let surface = hmap.surface[x][z] as usize;
            if surface <= SEA_LEVEL + 1 || surface + 14 >= CHUNK_H {
                continue;
            }
            if blocks[idx(x, surface, z)] != Block::Grass {
                continue;
            }
            let around_biomes = [
                hmap.biome[x - 1][z],
                hmap.biome[x + 1][z],
                hmap.biome[x][z - 1],
                hmap.biome[x][z + 1],
                hmap.biome[x - 1][z - 1],
                hmap.biome[x + 1][z - 1],
                hmap.biome[x - 1][z + 1],
                hmap.biome[x + 1][z + 1],
            ];
            let around_heights = [
                hmap.surface[x - 1][z],
                hmap.surface[x + 1][z],
                hmap.surface[x][z - 1],
                hmap.surface[x][z + 1],
                hmap.surface[x - 1][z - 1],
                hmap.surface[x + 1][z - 1],
                hmap.surface[x - 1][z + 1],
                hmap.surface[x + 1][z + 1],
            ];
            let slope = ((hmap.surface[x + 1][z] as i32 - hmap.surface[x - 1][z] as i32).abs())
                .max((hmap.surface[x][z + 1] as i32 - hmap.surface[x][z - 1] as i32).abs());
            let veg = biome::blended_vegetation_profile_weighted(
                b,
                around_biomes,
                hmap.surface[x][z],
                around_heights,
                slope,
                SEA_LEVEL as u32,
                hash2(seed ^ 0xA5A5_7F31, wx, wz),
            );
            let chance = veg.tree_chance;
            if chance <= 0.0 {
                continue;
            }
            let h = hash2(seed, wx, wz);
            let r = (h & 0xffff) as f32 / 65535.0;
            if r > chance {
                continue;
            }
            // Keep trees on playable/pleasant terrain.
            if slope >= 4 {
                continue;
            }
            let local_min = around_heights.into_iter().min().unwrap_or(hmap.surface[x][z]) as i32;
            let local_max = around_heights.into_iter().max().unwrap_or(hmap.surface[x][z]) as i32;
            let local_relief = local_max - local_min;
            if local_relief > 3 {
                continue;
            }
            if surface > SEA_LEVEL + 92 {
                continue;
            }
            let min_h = veg.min_height;
            let max_h = veg.max_height.max(min_h + 1);
            let height = min_h + ((h >> 16) as usize % (max_h - min_h + 1).max(1));
            let top = surface + height;
            let crown_radius = veg.crown_radius.clamp(1, 3);
            let crown_half_h = if crown_radius >= 3 { 3 } else { 2 };
            let crown_r = crown_radius as usize;
            if x < crown_r || z < crown_r || x + crown_r >= CHUNK_W || z + crown_r >= CHUNK_D {
                continue;
            }
            if top + (crown_half_h as usize) + 1 >= CHUNK_H {
                continue;
            }
            for y in (surface + 1)..=top {
                blocks[idx(x, y, z)] = Block::Log;
            }
            for dy in -crown_half_h..=crown_half_h {
                let y = top as i32 + dy;
                if y <= 0 || y >= CHUNK_H as i32 {
                    continue;
                }
                let radius = if dy.abs() == crown_half_h {
                    (crown_radius - 1).max(1)
                } else {
                    crown_radius
                };
                for dx in -radius..=radius {
                    for dz in -radius..=radius {
                        if dx == 0 && dz == 0 && dy <= 0 {
                            continue;
                        }
                        let manhattan: i32 = dx.abs() + dz.abs();
                        if manhattan > radius * 2 || (dy == crown_half_h && manhattan > radius) {
                            continue;
                        }
                        let xx = x as i32 + dx;
                        let zz = z as i32 + dz;
                        if xx < 0 || xx >= CHUNK_W as i32 || zz < 0 || zz >= CHUNK_D as i32 {
                            continue;
                        }
                        let li = idx(xx as usize, y as usize, zz as usize);
                        if blocks[li].is_air() {
                            blocks[li] = Block::Leaves;
                        }
                    }
                }
            }
        }
    }
}

fn place_boulders(blocks: &mut [Block], cx: i32, cz: i32, seed: u32, hmap: &Heightmap) {
    let world_x0 = cx * CHUNK_W as i32;
    let world_z0 = cz * CHUNK_D as i32;
    for x in 1..(CHUNK_W - 1) {
        let wx = world_x0 + x as i32;
        for z in 1..(CHUNK_D - 1) {
            let wz = world_z0 + z as i32;
            let biome = hmap.biome[x][z];
            let surface = hmap.surface[x][z] as usize;
            if surface <= SEA_LEVEL + 1 || surface + 6 >= CHUNK_H {
                continue;
            }

            let top = blocks[idx(x, surface, z)];
            if !matches!(top, Block::Grass | Block::Stone) {
                continue;
            }

            let chance = if biome::is_ocean(biome)
                || matches!(
                    biome,
                    Biome::Beach
                        | Biome::Desert
                        | Biome::Badlands
                        | Biome::ErodedBadlands
                        | Biome::WoodedBadlands
                )
            {
                0.0
            } else if biome::is_mountain(biome) {
                0.010
            } else {
                0.0025
            };
            if chance <= 0.0 {
                continue;
            }

            let h = hash2(seed ^ 0x9C1B_AA35, wx, wz);
            let roll = (h & 0xffff) as f32 / 65535.0;
            if roll > chance {
                continue;
            }

            let large = biome::is_mountain(biome) && ((h >> 22) & 0x7) == 0;
            let rx = if large { 2 } else { 1 };
            let rz = if large { 2 } else { 1 };
            let ry = 1;
            let center_y = surface as i32 + 1;
            let ore_roll = ((h >> 20) & 0xff) as f32 / 255.0;
            let boulder_block = if ore_roll < 0.07 {
                Block::CoalOre
            } else if ore_roll < 0.09 {
                Block::CopperOre
            } else {
                Block::Stone
            };

            for dx in -rx..=rx {
                for dz in -rz..=rz {
                    for dy in 0..=ry {
                        let nx = x as i32 + dx;
                        let ny = center_y + dy;
                        let nz = z as i32 + dz;
                        if nx <= 0
                            || nx >= CHUNK_W as i32 - 1
                            || nz <= 0
                            || nz >= CHUNK_D as i32 - 1
                            || ny <= 1
                            || ny >= CHUNK_H as i32 - 1
                        {
                            continue;
                        }

                        let ell = (dx as f32 * dx as f32) / ((rx * rx + 1) as f32)
                            + (dy as f32 * dy as f32) / ((ry * ry + 1) as f32)
                            + (dz as f32 * dz as f32) / ((rz * rz + 1) as f32);
                        if ell > 1.16 {
                            continue;
                        }

                        let i = idx(nx as usize, ny as usize, nz as usize);
                        if blocks[i].is_air() {
                            blocks[i] = boulder_block;
                        }
                    }
                }
            }
        }
    }
}

#[inline]
fn pick_stone_block(seed: u32, wx: i32, wy: i32, wz: i32, biome: Biome) -> Block {
    if wy < 1 {
        return Block::Stone;
    }

    let h = hash3(seed ^ 0xB529_7A4D, wx, wy, wz);
    let r = (h as f64) / (u32::MAX as f64);

    let mountain_bonus = if biome::is_mountain(biome) { 1.20 } else { 1.0 };

    if wy <= 26 && r < 0.016 * mountain_bonus {
        return Block::IronOre;
    }
    if wy <= 46 && r < 0.020 {
        return Block::CopperOre;
    }
    if wy <= 84 && r < 0.030 * mountain_bonus {
        return Block::CoalOre;
    }

    Block::Stone
}

fn hash2(seed: u32, x: i32, z: i32) -> u32 {
    let mut h = seed ^ (x as u32).wrapping_mul(374761393) ^ (z as u32).wrapping_mul(668265263);
    h = (h ^ (h >> 13)).wrapping_mul(1274126177);
    h ^ (h >> 16)
}

fn hash3(seed: u32, x: i32, y: i32, z: i32) -> u32 {
    let mut h = seed
        ^ (x as u32).wrapping_mul(0x9E37_79B9)
        ^ (y as u32).wrapping_mul(0x85EB_CA6B)
        ^ (z as u32).wrapping_mul(0xC2B2_AE35);
    h ^= h >> 16;
    h = h.wrapping_mul(0x7FEB_352D);
    h ^= h >> 15;
    h = h.wrapping_mul(0x846C_A68B);
    h ^ (h >> 16)
}

// ─── Vertex ───────────────────────────────────────────────────────────────────

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
pub struct Vertex {
    pub pos:     [f32; 3],
    pub normal:  [f32; 3],
    pub tex_idx: u32,
    pub uv:      [f32; 2],
}

impl Vertex {
    pub fn desc() -> wgpu::VertexBufferLayout<'static> {
        use std::mem;
        wgpu::VertexBufferLayout {
            array_stride: mem::size_of::<Vertex>() as u64,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &[
                wgpu::VertexAttribute { offset: 0,  shader_location: 0, format: wgpu::VertexFormat::Float32x3 },
                wgpu::VertexAttribute { offset: 12, shader_location: 1, format: wgpu::VertexFormat::Float32x3 },
                wgpu::VertexAttribute { offset: 24, shader_location: 2, format: wgpu::VertexFormat::Uint32    },
                wgpu::VertexAttribute { offset: 28, shader_location: 3, format: wgpu::VertexFormat::Float32x2 },
            ],
        }
    }
}



