use noise::{NoiseFn, Perlin, Fbm, SuperSimplex};
use bytemuck::{Pod, Zeroable};
use super::block::Block;

pub const CHUNK_W: usize = 16;
pub const CHUNK_H: usize = 256;
pub const CHUNK_D: usize = 16;
pub const SEA_LEVEL: usize = 62;

// ── Inline индекс для плоского массива (cache-friendly) ───────
#[inline(always)]
pub fn idx(x: usize, y: usize, z: usize) -> usize {
    x * CHUNK_H * CHUNK_D + y * CHUNK_D + z
}

// ── Карта высот — генерируется отдельно, дёшево ───────────────
#[derive(Clone)]
pub struct Heightmap {
    pub surface: Box<[[u32; CHUNK_D]; CHUNK_W]>, // высота поверхности
    pub biome:   Box<[[f32; CHUNK_D]; CHUNK_W]>, // 0=равнина 1=горы
}

impl Heightmap {
    pub fn generate(cx: i32, cz: i32, seed: u32) -> Self {
        let base        = Perlin::new(seed);
        let detail      = Perlin::new(seed.wrapping_add(1));
        let mountain_fbm: Fbm<Perlin> = {
            let mut f = Fbm::new(seed.wrapping_add(2));
            f.octaves    = 6;
            f.frequency  = 1.0;
            f.lacunarity = 2.0;
            f.persistence = 0.5;
            f
        };
        let biome_noise = SuperSimplex::new(seed.wrapping_add(3));

        let mut surface = Box::new([[0u32; CHUNK_D]; CHUNK_W]);
        let mut biome   = Box::new([[0f32; CHUNK_D]; CHUNK_W]);

        for x in 0..CHUNK_W {
            for z in 0..CHUNK_D {
                let wx = (cx * CHUNK_W as i32 + x as i32) as f64;
                let wz = (cz * CHUNK_D as i32 + z as i32) as f64;

                let b = biome_noise.get([wx / 400.0, wz / 400.0]);
                let t = ((b + 1.0) / 2.0).clamp(0.0, 1.0);
                let bv = smoothstep(smoothstep(t)) as f32;

                let plains_h = {
                    let n = base.get([wx / 180.0, wz / 180.0]);
                    let d = detail.get([wx / 40.0, wz / 40.0]) * 0.15;
                    62.0 + (n + d) * 10.0
                };
                let mountain_h = {
                    let raw    = mountain_fbm.get([wx / 200.0, wz / 200.0]);
                    let ridged = (1.0 - raw.abs() * 2.0).max(0.0).powf(1.5);
                    80.0 + ridged * 130.0
                };

                let h = (plains_h * (1.0 - bv as f64) + mountain_h * bv as f64) as u32;
                surface[x][z] = h.clamp(4, CHUNK_H as u32 - 2);
                biome[x][z]   = bv;
            }
        }

        Self { surface, biome }
    }

    /// Возвращает высоту с учётом блендинга с соседними heightmap'ами.
    /// Это сглаживает стыки на границах чанков.
    pub fn blended_surface(
        &self,
        x: usize, z: usize,
        neighbors: [Option<&Heightmap>; 4], // px nx pz nz
    ) -> u32 {
        let base = self.surface[x][z] as f32;
        let mut sum    = base;
        let mut weight = 1.0f32;

        // Чем ближе к границе — тем сильнее тянем к соседу
        let blend_w = CHUNK_W as f32;
        let blend_d = CHUNK_D as f32;

        // +X граница
        if x >= CHUNK_W - 2 {
            if let Some(nb) = neighbors[1] {
                let t = (x as f32 - (CHUNK_W - 2) as f32) / 2.0;
                let nb_h = nb.surface[0][z] as f32;
                sum    += nb_h * t;
                weight += t;
            }
        }
        // -X граница
        if x <= 1 {
            if let Some(nb) = neighbors[0] {
                let t = 1.0 - x as f32 / 2.0;
                let nb_h = nb.surface[CHUNK_W - 1][z] as f32;
                sum    += nb_h * t;
                weight += t;
            }
        }
        // +Z граница
        if z >= CHUNK_D - 2 {
            if let Some(nb) = neighbors[3] {
                let t = (z as f32 - (CHUNK_D - 2) as f32) / 2.0;
                let nb_h = nb.surface[x][0] as f32;
                sum    += nb_h * t;
                weight += t;
            }
        }
        // -Z граница
        if z <= 1 {
            if let Some(nb) = neighbors[2] {
                let t = 1.0 - z as f32 / blend_d * 2.0;
                let nb_h = nb.surface[x][CHUNK_D - 1] as f32;
                sum    += nb_h * t;
                weight += t;
            }
        }
        let _ = (blend_w, blend_d); // suppress warnings

        ((sum / weight) as u32).clamp(4, CHUNK_H as u32 - 2)
    }
}

fn smoothstep(t: f64) -> f64 { t * t * (3.0 - 2.0 * t) }

// ── Чанк ──────────────────────────────────────────────────────
use std::sync::Arc;

#[derive(Clone)]
pub struct Chunk {
    pub cx: i32,
    pub cz: i32,
    // Arc — клон это просто +1 к счётчику, не копия данных
    pub blocks: Arc<[Block]>,
    pub heightmap: Arc<Heightmap>,
}

impl Chunk {
    pub fn generate(
        cx: i32, cz: i32, seed: u32,
        hmap: Heightmap,
        neighbor_hmaps: [Option<&Heightmap>; 4],
    ) -> Self {
        let cave_noise = Perlin::new(seed.wrapping_add(4));

        let size = CHUNK_W * CHUNK_H * CHUNK_D;
        let mut blocks = vec![Block::Air; size].into_boxed_slice();

        for x in 0..CHUNK_W {
            for z in 0..CHUNK_D {
                let surface = hmap.blended_surface(x, z, neighbor_hmaps) as usize;
                let biome   = hmap.biome[x][z];

                let wx = (cx * CHUNK_W as i32 + x as i32) as f64;
                let wz = (cz * CHUNK_D as i32 + z as i32) as f64;

                // Максимальная высота в этой колонке — выше всё Air, пропускаем
                let col_max = surface.max(SEA_LEVEL) + 1;

                for y in 0..col_max.min(CHUNK_H) {
                    let i = idx(x, y, z);
                    blocks[i] = if y == 0 {
                        Block::Bedrock
                    } else if y < surface.saturating_sub(4) {
                        // Пещеры
                        let cave = cave_noise.get([wx / 20.0, y as f64 / 20.0, wz / 20.0]);
                        if cave > 0.65 && y > 5 { Block::Air } else { Block::Stone }
                    } else if y < surface {
                        Block::Dirt
                    } else if y == surface {
                        if surface <= SEA_LEVEL {
                            Block::Sand
                        } else if biome > 0.7 && surface > 110 {
                            Block::Stone
                        } else {
                            Block::Grass
                        }
                    } else {
                        // Вода до уровня моря
                        if y <= SEA_LEVEL { Block::Water } else { Block::Air }
                    };
                }
            }
        }

        Self { cx, cz, blocks: blocks.into(), heightmap: Arc::new(hmap) }
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

        const FACES: [([f32;3], [[f32;3];4]); 6] = [
            ([1.,0.,0.],  [[1.,0.,0.],[1.,1.,0.],[1.,1.,1.],[1.,0.,1.]]),
            ([-1.,0.,0.], [[0.,0.,1.],[0.,1.,1.],[0.,1.,0.],[0.,0.,0.]]),
            ([0.,1.,0.],  [[0.,1.,0.],[0.,1.,1.],[1.,1.,1.],[1.,1.,0.]]),
            ([0.,-1.,0.], [[0.,0.,1.],[0.,0.,0.],[1.,0.,0.],[1.,0.,1.]]),
            ([0.,0.,1.],  [[1.,0.,1.],[1.,1.,1.],[0.,1.,1.],[0.,0.,1.]]),
            ([0.,0.,-1.], [[0.,0.,0.],[0.,1.,0.],[1.,1.,0.],[1.,0.,0.]]),
        ];

        let ox = (self.cx * CHUNK_W as i32) as f32;
        let oz = (self.cz * CHUNK_D as i32) as f32;

        for x in 0..CHUNK_W {
            for z in 0..CHUNK_D {
                // Верхняя граница колонки из heightmap — пропускаем воздух выше
                let col_top = (self.heightmap.surface[x][z] as usize + 1)
                    .max(SEA_LEVEL + 1)
                    .min(CHUNK_H);

                for y in 0..col_top {
                    let block = self.get(x, y, z);
                    if !block.is_solid() { continue; }

                    let tex = block.texture_index();

                    let solid = |bx: i32, by: i32, bz: i32| -> bool {
                        if by < 0 || by >= CHUNK_H as i32 { return by < 0; }
                        let (bx, by, bz) = (bx as usize, by as usize, bz as usize);
                        if bx < CHUNK_W && bz < CHUNK_D {
                            return self.get(bx, by, bz).is_solid();
                        }
                        // Соседние чанки
                        if bx >= CHUNK_W {
                            return neighbor_nx.map_or(false, |c| c.get(0, by, bz).is_solid());
                        }
                        if bz >= CHUNK_D {
                            return neighbor_nz.map_or(false, |c| c.get(bx, by, 0).is_solid());
                        }
                        // bx == usize::MAX (underflow от -1)
                        if bx > CHUNK_W {
                            return neighbor_px.map_or(false, |c| c.get(CHUNK_W-1, by, bz).is_solid());
                        }
                        neighbor_pz.map_or(false, |c| c.get(bx, by, CHUNK_D-1).is_solid())
                    };

                    let xi = x as i32;
                    let yi = y as i32;
                    let zi = z as i32;

                    let neighbors = [
                        solid(xi+1, yi, zi),
                        solid(xi-1, yi, zi),
                        solid(xi, yi+1, zi),
                        solid(xi, yi-1, zi),
                        solid(xi, yi, zi+1),
                        solid(xi, yi, zi-1),
                    ];

                    for (i, (normal, corners)) in FACES.iter().enumerate() {
                        if neighbors[i] { continue; }
                        let v: [Vertex; 4] = std::array::from_fn(|j| Vertex {
                            pos:     [ox + x as f32 + corners[j][0],
                                           y as f32 + corners[j][1],
                                      oz + z as f32 + corners[j][2]],
                            normal:  *normal,
                            tex_idx: tex,
                        });
                        verts.extend_from_slice(&[v[0],v[1],v[2], v[0],v[2],v[3]]);
                    }
                }
            }
        }

        verts
    }
}

// ── Vertex ────────────────────────────────────────────────────
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
pub struct Vertex {
    pub pos:     [f32; 3],
    pub normal:  [f32; 3],
    pub tex_idx: u32,
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
                wgpu::VertexAttribute { offset: 24, shader_location: 2, format: wgpu::VertexFormat::Uint32 },
            ],
        }
    }
}