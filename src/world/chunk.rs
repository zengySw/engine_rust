use noise::{Fbm, NoiseFn, Perlin, SuperSimplex};
use bytemuck::{Pod, Zeroable};
use super::block::Block;
use std::sync::Arc;

pub const CHUNK_W: usize = 16;
pub const CHUNK_H: usize = 256;
pub const CHUNK_D: usize = 16;
pub const SEA_LEVEL: usize = 62;
pub const SPAWN_PLANE_HALF: i32 = 7;
const SPAWN_PLANE_CLEAR_HALF: i32 = 9;

#[inline(always)]
pub fn idx(x: usize, y: usize, z: usize) -> usize {
    x * CHUNK_H * CHUNK_D + y * CHUNK_D + z
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Biome {
    Ocean,
    Beach,
    Plains,
    Forest,
    Desert,
    Mountain,
}

#[derive(Clone)]
pub struct Heightmap {
    pub surface: Box<[[u32; CHUNK_D]; CHUNK_W]>,
    pub biome: Box<[[Biome; CHUNK_D]; CHUNK_W]>,
}

struct TerrainSampler {
    continental: Perlin,
    erosion: Fbm<Perlin>,
    peaks: Fbm<Perlin>,
    detail: Perlin,
    temperature: SuperSimplex,
    moisture: SuperSimplex,
}

impl TerrainSampler {
    fn new(seed: u32) -> Self {
        let continental = Perlin::new(seed.wrapping_add(10));
        let detail = Perlin::new(seed.wrapping_add(11));

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

        Self {
            continental,
            erosion,
            peaks,
            detail,
            temperature,
            moisture,
        }
    }

    fn sample_surface_and_biome(&self, wx: f64, wz: f64) -> (u32, Biome) {
        let continental = remap01(self.continental.get([wx / 980.0, wz / 980.0]));
        let erosion = remap01(self.erosion.get([wx / 450.0, wz / 450.0]));
        let peaks_raw = self.peaks.get([wx / 280.0, wz / 280.0]);
        let detail = self.detail.get([wx / 140.0, wz / 140.0]);

        let land = smoothstep(remap01((continental - 0.15) * 1.2));
        let ridged = (1.0 - (peaks_raw * 1.6).abs()).clamp(0.0, 1.0).powf(1.8);
        let mountain_mask = smoothstep(((land - 0.50) / 0.45).clamp(0.0, 1.0));

        let base = SEA_LEVEL as f64 - 24.0 + land * 58.0;
        let hills = detail * (6.0 + (1.0 - erosion) * 8.0);
        let mountain = ridged * mountain_mask * (45.0 + (1.0 - erosion) * 50.0);
        let height_f = (base + hills + mountain).clamp(4.0, (CHUNK_H - 2) as f64);
        let surface = height_f.round() as u32;

        let mut temperature = remap01(self.temperature.get([wx / 720.0, wz / 720.0]));
        temperature -= ((height_f - SEA_LEVEL as f64).max(0.0)) * 0.0026;
        let moisture = remap01(self.moisture.get([wx / 660.0, wz / 660.0]));

        let biome = if surface <= SEA_LEVEL as u32 - 3 {
            Biome::Ocean
        } else if surface <= SEA_LEVEL as u32 + 1 {
            Biome::Beach
        } else if surface >= 122 || (surface >= 98 && ridged > 0.58 && land > 0.56) {
            Biome::Mountain
        } else if temperature > 0.68 && moisture < 0.35 {
            Biome::Desert
        } else if moisture > 0.60 {
            Biome::Forest
        } else {
            Biome::Plains
        };

        (surface, biome)
    }
}

impl Heightmap {
    pub fn generate(cx: i32, cz: i32, seed: u32) -> Self {
        let sampler = TerrainSampler::new(seed);

        let mut surface = Box::new([[0u32; CHUNK_D]; CHUNK_W]);
        let mut biome = Box::new([[Biome::Plains; CHUNK_D]; CHUNK_W]);
        let spawn_plane_y = spawn_plane_base_y(seed) as u32;

        for x in 0..CHUNK_W {
            for z in 0..CHUNK_D {
                let wx = (cx * CHUNK_W as i32 + x as i32) as f64;
                let wz = (cz * CHUNK_D as i32 + z as i32) as f64;

                let (mut h, mut b) = sampler.sample_surface_and_biome(wx, wz);
                if in_spawn_plane_area(wx as i32, wz as i32, 0) {
                    h = spawn_plane_y;
                    b = Biome::Plains;
                }

                surface[x][z] = h;
                biome[x][z] = b;
            }
        }

        Self { surface, biome }
    }

    pub fn blended_surface(
        &self,
        x: usize,
        z: usize,
        neighbors: [Option<&Heightmap>; 4],
    ) -> u32 {
        let base = self.surface[x][z] as f32;
        let mut sum = base;
        let mut weight = 1.0f32;

        if x >= CHUNK_W - 2 {
            if let Some(nb) = neighbors[1] {
                let t = (x as f32 - (CHUNK_W - 2) as f32) / 2.0;
                let nb_h = nb.surface[0][z] as f32;
                sum += nb_h * t;
                weight += t;
            }
        }
        if x <= 1 {
            if let Some(nb) = neighbors[0] {
                let t = 1.0 - x as f32 / 2.0;
                let nb_h = nb.surface[CHUNK_W - 1][z] as f32;
                sum += nb_h * t;
                weight += t;
            }
        }
        if z >= CHUNK_D - 2 {
            if let Some(nb) = neighbors[3] {
                let t = (z as f32 - (CHUNK_D - 2) as f32) / 2.0;
                let nb_h = nb.surface[x][0] as f32;
                sum += nb_h * t;
                weight += t;
            }
        }
        if z <= 1 {
            if let Some(nb) = neighbors[2] {
                let t = 1.0 - z as f32 / 2.0;
                let nb_h = nb.surface[x][CHUNK_D - 1] as f32;
                sum += nb_h * t;
                weight += t;
            }
        }

        ((sum / weight) as u32).clamp(4, CHUNK_H as u32 - 2)
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

pub fn spawn_plane_floor_y(seed: u32) -> f32 {
    spawn_plane_base_y(seed) as f32 + 2.05
}

#[derive(Clone)]
pub struct Chunk {
    pub cx: i32,
    pub cz: i32,
    pub blocks: Arc<[Block]>,
    pub heightmap: Arc<Heightmap>,
}

impl Chunk {
    pub fn generate(
        cx: i32,
        cz: i32,
        seed: u32,
        hmap: Heightmap,
        _neighbor_hmaps: [Option<&Heightmap>; 4],
    ) -> Self {
        let cave_a = Perlin::new(seed.wrapping_add(20));
        let cave_b = Perlin::new(seed.wrapping_add(21));

        let size = CHUNK_W * CHUNK_H * CHUNK_D;
        let mut blocks = vec![Block::Air; size].into_boxed_slice();

        for x in 0..CHUNK_W {
            for z in 0..CHUNK_D {
                let surface = hmap.surface[x][z] as usize;
                let biome = hmap.biome[x][z];

                let xm = x.saturating_sub(1);
                let xp = (x + 1).min(CHUNK_W - 1);
                let zm = z.saturating_sub(1);
                let zp = (z + 1).min(CHUNK_D - 1);
                let sxm = hmap.surface[xm][z] as i32;
                let sxp = hmap.surface[xp][z] as i32;
                let szm = hmap.surface[x][zm] as i32;
                let szp = hmap.surface[x][zp] as i32;
                let slope = (sxp - sxm).abs().max((szp - szm).abs());

                let wx = (cx * CHUNK_W as i32 + x as i32) as f64;
                let wz = (cz * CHUNK_D as i32 + z as i32) as f64;

                let beach = matches!(biome, Biome::Ocean | Biome::Beach);
                let mut top_block = match biome {
                    Biome::Ocean | Biome::Beach | Biome::Desert => Block::Sand,
                    Biome::Mountain => Block::Stone,
                    Biome::Plains | Biome::Forest => Block::Grass,
                };
                let mut filler_block = match biome {
                    Biome::Ocean | Biome::Beach | Biome::Desert => Block::Sand,
                    Biome::Mountain => Block::Stone,
                    Biome::Plains | Biome::Forest => Block::Dirt,
                };
                let mut filler_depth = match biome {
                    Biome::Ocean | Biome::Beach | Biome::Desert => 4,
                    Biome::Mountain => 5,
                    Biome::Forest => 4,
                    Biome::Plains => 3,
                };

                let cliff_rock = surface > SEA_LEVEL + 6 && slope >= 3;
                let alpine_rock = surface > 120;
                if cliff_rock || alpine_rock {
                    top_block = Block::Stone;
                    filler_block = Block::Stone;
                    filler_depth = filler_depth.max(if alpine_rock { 6 } else { 4 });
                }

                let col_max = surface.max(SEA_LEVEL) + 1;
                for y in 0..col_max.min(CHUNK_H) {
                    let i = idx(x, y, z);
                    blocks[i] = if y == 0 {
                        Block::Bedrock
                    } else if y + filler_depth < surface {
                        let can_cave = y > 8 && y < surface.saturating_sub(8);
                        if can_cave {
                            let n0 = cave_a.get([wx / 26.0, y as f64 / 22.0, wz / 26.0]);
                            let n1 = cave_b.get([wx / 14.0, y as f64 / 14.0, wz / 14.0]).abs();
                            let threshold = if matches!(biome, Biome::Mountain) { 0.57 } else { 0.62 };
                            if n0 > threshold && n1 > 0.33 {
                                Block::Air
                            } else {
                                Block::Stone
                            }
                        } else {
                            Block::Stone
                        }
                    } else if y < surface {
                        filler_block
                    } else if y == surface {
                        top_block
                    } else if y <= SEA_LEVEL {
                        if beach || matches!(biome, Biome::Ocean) {
                            Block::Water
                        } else {
                            Block::Water
                        }
                    } else {
                        Block::Air
                    };
                }
            }
        }

        place_trees(&mut blocks, cx, cz, seed, &hmap);
        place_spawn_plane(&mut blocks, cx, cz, seed);

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
            ([1., 0., 0.], [[1., 0., 0.], [1., 1., 0.], [1., 1., 1.], [1., 0., 1.]]),
            ([-1., 0., 0.], [[0., 0., 1.], [0., 1., 1.], [0., 1., 0.], [0., 0., 0.]]),
            ([0., 1., 0.], [[0., 1., 0.], [0., 1., 1.], [1., 1., 1.], [1., 1., 0.]]),
            ([0., -1., 0.], [[0., 0., 1.], [0., 0., 0.], [1., 0., 0.], [1., 0., 1.]]),
            ([0., 0., 1.], [[1., 0., 1.], [1., 1., 1.], [0., 1., 1.], [0., 0., 1.]]),
            ([0., 0., -1.], [[0., 0., 0.], [0., 1., 0.], [1., 1., 0.], [1., 0., 0.]]),
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
                        if self.get(x, y, z) != Block::Air {
                            col_top = (y + 1).min(CHUNK_H);
                            break;
                        }
                    }
                }

                for y in 0..col_top {
                    let block = self.get(x, y, z);
                    if !block.is_solid() {
                        continue;
                    }

                    let solid = |bx: i32, by: i32, bz: i32| -> bool {
                        if by < 0 || by >= CHUNK_H as i32 {
                            return by < 0;
                        }
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
                        solid(xi + 1, yi, zi),
                        solid(xi - 1, yi, zi),
                        solid(xi, yi + 1, zi),
                        solid(xi, yi - 1, zi),
                        solid(xi, yi, zi + 1),
                        solid(xi, yi, zi - 1),
                    ];

                    for (i, (normal, corners)) in FACES.iter().enumerate() {
                        if neighbors[i] {
                            continue;
                        }
                        let tex = face_texture_for(block, i);
                        let uvs = [[0.0, 1.0], [0.0, 0.0], [1.0, 0.0], [1.0, 1.0]];
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
                        verts.extend_from_slice(&[v[0], v[1], v[2], v[0], v[2], v[3]]);
                    }
                }
            }
        }

        verts
    }
}

#[inline]
fn face_texture_for(block: Block, face_idx: usize) -> u32 {
    match block {
        Block::Grass => {
            if face_idx == 2 {
                Block::Grass.texture_index()
            } else {
                Block::Dirt.texture_index()
            }
        }
        Block::Log => {
            if face_idx == 2 || face_idx == 3 {
                Block::LogBottom.texture_index()
            } else {
                Block::Log.texture_index()
            }
        }
        _ => block.texture_index(),
    }
}

fn place_trees(blocks: &mut [Block], cx: i32, cz: i32, seed: u32, hmap: &Heightmap) {
    let margin = 2;
    for x in margin..(CHUNK_W - margin) {
        for z in margin..(CHUNK_D - margin) {
            let surface = hmap.surface[x][z] as usize;
            if surface <= SEA_LEVEL + 1 || surface + 8 >= CHUNK_H {
                continue;
            }
            if blocks[idx(x, surface, z)] != Block::Grass {
                continue;
            }

            let biome = hmap.biome[x][z];
            if !matches!(biome, Biome::Plains | Biome::Forest) {
                continue;
            }

            let xm = x.saturating_sub(1);
            let xp = (x + 1).min(CHUNK_W - 1);
            let zm = z.saturating_sub(1);
            let zp = (z + 1).min(CHUNK_D - 1);
            let sxm = hmap.surface[xm][z] as i32;
            let sxp = hmap.surface[xp][z] as i32;
            let szm = hmap.surface[x][zm] as i32;
            let szp = hmap.surface[x][zp] as i32;
            let slope = (sxp - sxm).abs().max((szp - szm).abs());
            if slope >= 3 {
                continue;
            }

            let wx = cx * CHUNK_W as i32 + x as i32;
            let wz = cz * CHUNK_D as i32 + z as i32;
            if in_spawn_plane_area(wx, wz, 2) {
                continue;
            }

            let h = hash2(seed, wx, wz);
            let r = (h & 0xffff) as f32 / 65535.0;
            let chance = match biome {
                Biome::Forest => 0.060,
                Biome::Plains => 0.022,
                _ => 0.0,
            };
            if r > chance {
                continue;
            }

            let height = match biome {
                Biome::Forest => 5 + ((h >> 16) % 3) as usize,
                _ => 4 + ((h >> 16) % 2) as usize,
            };
            let top = surface + height;
            if top + 2 >= CHUNK_H {
                continue;
            }

            for y in (surface + 1)..=top {
                blocks[idx(x, y, z)] = Block::Log;
            }

            for dy in -2..=2 {
                let y = top as i32 + dy;
                if y <= 0 || y >= CHUNK_H as i32 {
                    continue;
                }
                let radius: i32 = if dy.abs() == 2 { 1 } else { 2 };
                for dx in -radius..=radius {
                    for dz in -radius..=radius {
                        if dx == 0 && dz == 0 && dy <= 0 {
                            continue;
                        }
                        let manhattan = dx.abs() + dz.abs();
                        if manhattan > radius * 2 || (dy == 2 && manhattan > 1) {
                            continue;
                        }

                        let xx = x as i32 + dx;
                        let zz = z as i32 + dz;
                        if xx < 0 || xx >= CHUNK_W as i32 || zz < 0 || zz >= CHUNK_D as i32 {
                            continue;
                        }
                        let i = idx(xx as usize, y as usize, zz as usize);
                        if blocks[i] == Block::Air {
                            blocks[i] = Block::Leaves;
                        }
                    }
                }
            }
        }
    }
}

fn place_spawn_plane(blocks: &mut [Block], cx: i32, cz: i32, seed: u32) {
    if cx.abs() > 1 || cz.abs() > 1 {
        return;
    }

    let base_y = spawn_plane_base_y(seed);
    let floor_half = SPAWN_PLANE_HALF;

    for wx in -SPAWN_PLANE_CLEAR_HALF..=SPAWN_PLANE_CLEAR_HALF {
        for wz in -SPAWN_PLANE_CLEAR_HALF..=SPAWN_PLANE_CLEAR_HALF {
            for wy in (base_y + 2)..=(base_y + 20) {
                set_world_block_in_chunk(blocks, cx, cz, wx, wy, wz, Block::Air);
            }
            if wx.abs() <= floor_half + 1 && wz.abs() <= floor_half + 1 {
                for wy in (base_y - 3)..=base_y {
                    set_world_block_in_chunk(blocks, cx, cz, wx, wy, wz, Block::Stone);
                }
            }
        }
    }

    for wx in -floor_half..=floor_half {
        for wz in -floor_half..=floor_half {
            set_world_block_in_chunk(blocks, cx, cz, wx, base_y + 1, wz, Block::Grass);
            set_world_block_in_chunk(blocks, cx, cz, wx, base_y, wz, Block::Dirt);
            set_world_block_in_chunk(blocks, cx, cz, wx, base_y - 1, wz, Block::Dirt);
        }
    }

    for wx in -(floor_half + 1)..=(floor_half + 1) {
        for wz in -(floor_half + 1)..=(floor_half + 1) {
            if wx.abs() == floor_half + 1 || wz.abs() == floor_half + 1 {
                set_world_block_in_chunk(blocks, cx, cz, wx, base_y + 1, wz, Block::Stone);
            }
        }
    }
}

fn set_world_block_in_chunk(
    blocks: &mut [Block],
    cx: i32,
    cz: i32,
    wx: i32,
    wy: i32,
    wz: i32,
    block: Block,
) {
    if wy < 1 || wy >= CHUNK_H as i32 {
        return;
    }
    let target_cx = wx.div_euclid(CHUNK_W as i32);
    let target_cz = wz.div_euclid(CHUNK_D as i32);
    if target_cx != cx || target_cz != cz {
        return;
    }
    let lx = wx.rem_euclid(CHUNK_W as i32) as usize;
    let lz = wz.rem_euclid(CHUNK_D as i32) as usize;
    blocks[idx(lx, wy as usize, lz)] = block;
}

fn in_spawn_plane_area(wx: i32, wz: i32, margin: i32) -> bool {
    wx.abs() <= SPAWN_PLANE_CLEAR_HALF + margin && wz.abs() <= SPAWN_PLANE_CLEAR_HALF + margin
}

fn spawn_plane_base_y(seed: u32) -> i32 {
    let sampler = TerrainSampler::new(seed);
    let (height, _) = sampler.sample_surface_and_biome(0.0, 0.0);
    (height as i32).clamp(8, CHUNK_H as i32 - 20)
}

fn hash2(seed: u32, x: i32, z: i32) -> u32 {
    let mut h = seed ^ (x as u32).wrapping_mul(374761393) ^ (z as u32).wrapping_mul(668265263);
    h = (h ^ (h >> 13)).wrapping_mul(1274126177);
    h ^ (h >> 16)
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
pub struct Vertex {
    pub pos: [f32; 3],
    pub normal: [f32; 3],
    pub tex_idx: u32,
    pub uv: [f32; 2],
}

impl Vertex {
    pub fn desc() -> wgpu::VertexBufferLayout<'static> {
        use std::mem;
        wgpu::VertexBufferLayout {
            array_stride: mem::size_of::<Vertex>() as u64,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &[
                wgpu::VertexAttribute {
                    offset: 0,
                    shader_location: 0,
                    format: wgpu::VertexFormat::Float32x3,
                },
                wgpu::VertexAttribute {
                    offset: 12,
                    shader_location: 1,
                    format: wgpu::VertexFormat::Float32x3,
                },
                wgpu::VertexAttribute {
                    offset: 24,
                    shader_location: 2,
                    format: wgpu::VertexFormat::Uint32,
                },
                wgpu::VertexAttribute {
                    offset: 28,
                    shader_location: 3,
                    format: wgpu::VertexFormat::Float32x2,
                },
            ],
        }
    }
}
