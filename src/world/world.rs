use std::collections::{HashMap, HashSet};
use std::sync::mpsc::{self, Receiver, SyncSender};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use super::biome::Biome;
use super::block::Block;
use super::chunk::{idx, Chunk, Vertex, CHUNK_D, CHUNK_W};
use crate::args::Args;
use crate::save;

const CARDINAL_NEIGHBORS: [(i32, i32); 4] = [(1, 0), (-1, 0), (0, 1), (0, -1)];
const MAX_GEN_RESULTS_PER_UPDATE: usize = 6;
const MAX_MESH_RESULTS_PER_UPDATE: usize = 24;
const MAX_MESH_TASKS_PER_UPDATE: usize = 24;
const MAX_IN_FLIGHT_GEN_JOBS: usize = 64;
const MAX_IN_FLIGHT_MESH_JOBS: usize = 96;
const WORLD_SAVE_COOLDOWN_SECS: f32 = 0.60;

pub struct ChunkMesh {
    pub verts: Vec<Vertex>,
}

struct MeshTask {
    key: (i32, i32),
    chunk: Arc<Chunk>,
    px: Option<Arc<Chunk>>,
    nx: Option<Arc<Chunk>>,
    pz: Option<Arc<Chunk>>,
    nz: Option<Arc<Chunk>>,
}

pub struct World {
    pub chunks: HashMap<(i32, i32), Arc<Chunk>>,
    seed: u32,
    world_type: save::WorldType,
    render_dist: i32,
    modified_blocks: save::BlockMap,
    pending_block_mods: HashMap<(i32, i32), Vec<(i32, i32, i32, Block)>>,
    save_dirty: bool,
    last_save_at: Instant,

    gen_tx: SyncSender<(i32, i32, u32, save::WorldType)>,
    gen_rx: Receiver<((i32, i32), Chunk)>,
    in_flight_gen: HashSet<(i32, i32)>,

    mesh_tx: SyncSender<MeshTask>,
    mesh_rx: Receiver<((i32, i32), ChunkMesh)>,
    in_flight_mesh: HashSet<(i32, i32)>,
    dirty_mesh: HashSet<(i32, i32)>,

    pub ready_meshes: Vec<((i32, i32), ChunkMesh)>,
    pub removed: Vec<(i32, i32)>,
}

impl World {
    pub fn new(seed: u32, args: &Args) -> Self {
        let render_dist = args.render_dist;
        let world_type = save::world_type_for_seed(seed);

        // Chunk generation pool (direct generation, no pre-heightmap stage).
        let (gen_tx, gen_task_rx) = mpsc::sync_channel::<(i32, i32, u32, save::WorldType)>(64);
        let (gen_result_tx, gen_rx) = mpsc::sync_channel::<((i32, i32), Chunk)>(64);
        let gen_pool = rayon::ThreadPoolBuilder::new()
            .num_threads(args.gen_threads())
            .thread_name(|i| format!("gen-{i}"))
            .build()
            .unwrap();
        let gen_tx2 = Arc::new(Mutex::new(gen_result_tx));

        std::thread::spawn(move || {
            while let Ok((cx, cz, seed, world_type)) = gen_task_rx.recv() {
                let tx = Arc::clone(&gen_tx2);
                gen_pool.spawn(move || {
                    let chunk = Chunk::generate(cx, cz, seed, world_type);
                    let _ = tx.lock().unwrap().send(((cx, cz), chunk));
                });
            }
        });

        // Mesh pool.
        let (mesh_tx, mesh_task_rx) = mpsc::sync_channel::<MeshTask>(64);
        let (mesh_result_tx, mesh_rx) = mpsc::sync_channel::<((i32, i32), ChunkMesh)>(64);
        let mesh_pool = rayon::ThreadPoolBuilder::new()
            .num_threads(args.mesh_threads())
            .thread_name(|i| format!("mesh-{i}"))
            .build()
            .unwrap();
        let mesh_tx2 = Arc::new(Mutex::new(mesh_result_tx));

        std::thread::spawn(move || {
            while let Ok(task) = mesh_task_rx.recv() {
                let tx = Arc::clone(&mesh_tx2);
                mesh_pool.spawn(move || {
                    let verts = task.chunk.build_mesh(
                        task.px.as_deref(),
                        task.nx.as_deref(),
                        task.pz.as_deref(),
                        task.nz.as_deref(),
                    );
                    let _ = tx.lock().unwrap().send((task.key, ChunkMesh { verts }));
                });
            }
        });

        let mut world = Self {
            chunks: HashMap::new(),
            seed,
            world_type,
            render_dist,
            modified_blocks: HashMap::new(),
            pending_block_mods: HashMap::new(),
            save_dirty: false,
            last_save_at: Instant::now(),
            gen_tx,
            gen_rx,
            in_flight_gen: HashSet::new(),
            mesh_tx,
            mesh_rx,
            in_flight_mesh: HashSet::new(),
            dirty_mesh: HashSet::new(),
            ready_meshes: Vec::new(),
            removed: Vec::new(),
        };

        world.load_saved_blocks();
        world
    }

    pub fn update(&mut self, player_x: f32, player_z: f32) {
        let cx = (player_x / CHUNK_W as f32).floor() as i32;
        let cz = (player_z / CHUNK_D as f32).floor() as i32;
        self.queue_chunk_generation(cx, cz);
        self.receive_generated_chunks();
        self.queue_dirty_meshes();
        self.receive_ready_meshes();
        self.unload_far_chunks(cx, cz);
    }

    pub fn chunk_count(&self) -> usize {
        self.chunks.len()
    }

    pub fn set_render_distance(&mut self, render_dist: i32) {
        self.render_dist = render_dist.clamp(2, 32);
    }

    pub fn save_if_dirty(&mut self) {
        if !self.save_dirty {
            return;
        }
        if self.last_save_at.elapsed().as_secs_f32() < WORLD_SAVE_COOLDOWN_SECS {
            return;
        }
        save::save_world_blocks(self.seed, &self.modified_blocks);
        self.save_dirty = false;
        self.last_save_at = Instant::now();
    }

    pub fn save_all(&self) {
        save::save_world_blocks(self.seed, &self.modified_blocks);
    }

    pub fn drain_ready_meshes(&mut self, max: usize) -> Vec<((i32, i32), ChunkMesh)> {
        let n = self.ready_meshes.len().min(max);
        if n == 0 {
            return Vec::new();
        }
        self.ready_meshes.drain(..n).collect()
    }

    pub fn biome_at_world(&self, wx: i32, wz: i32) -> Option<Biome> {
        let cx = wx.div_euclid(CHUNK_W as i32);
        let cz = wz.div_euclid(CHUNK_D as i32);
        let lx = wx.rem_euclid(CHUNK_W as i32) as usize;
        let lz = wz.rem_euclid(CHUNK_D as i32) as usize;

        self.chunks
            .get(&(cx, cz))
            .map(|chunk| chunk.heightmap.biome[lx][lz])
    }

    pub fn surface_at_world(&self, wx: i32, wz: i32) -> Option<u32> {
        let cx = wx.div_euclid(CHUNK_W as i32);
        let cz = wz.div_euclid(CHUNK_D as i32);
        let lx = wx.rem_euclid(CHUNK_W as i32) as usize;
        let lz = wz.rem_euclid(CHUNK_D as i32) as usize;

        self.chunks
            .get(&(cx, cz))
            .map(|chunk| chunk.heightmap.surface[lx][lz])
    }

    pub fn block_at_world(&self, wx: i32, wy: i32, wz: i32) -> Block {
        if wy < 0 {
            return Block::Bedrock;
        }
        if wy >= super::chunk::CHUNK_H as i32 {
            return Block::Air;
        }

        let cx = wx.div_euclid(CHUNK_W as i32);
        let cz = wz.div_euclid(CHUNK_D as i32);
        let lx = wx.rem_euclid(CHUNK_W as i32) as usize;
        let lz = wz.rem_euclid(CHUNK_D as i32) as usize;

        self.chunks
            .get(&(cx, cz))
            .map_or(Block::Air, |chunk| chunk.get(lx, wy as usize, lz))
    }

    pub fn set_block_at_world(&mut self, wx: i32, wy: i32, wz: i32, block: Block) -> bool {
        if wy < 0 || wy >= super::chunk::CHUNK_H as i32 {
            return false;
        }

        let cx = wx.div_euclid(CHUNK_W as i32);
        let cz = wz.div_euclid(CHUNK_D as i32);
        let lx = wx.rem_euclid(CHUNK_W as i32) as usize;
        let lz = wz.rem_euclid(CHUNK_D as i32) as usize;
        let key = (cx, cz);

        let Some(chunk_arc) = self.chunks.get(&key).cloned() else {
            self.upsert_pending_block_mod(key, wx, wy, wz, block);
            self.modified_blocks.insert((wx, wy, wz), block);
            self.save_dirty = true;
            return true;
        };

        let current = chunk_arc.get(lx, wy as usize, lz);
        if current == block {
            return true;
        }

        let mut blocks = chunk_arc.blocks.to_vec().into_boxed_slice();
        blocks[idx(lx, wy as usize, lz)] = block;

        let new_chunk = Chunk {
            cx: chunk_arc.cx,
            cz: chunk_arc.cz,
            blocks: blocks.into(),
            heightmap: Arc::clone(&chunk_arc.heightmap),
        };
        self.chunks.insert(key, Arc::new(new_chunk));
        self.mark_chunk_and_neighbors_dirty(key);
        self.upsert_pending_block_mod(key, wx, wy, wz, block);
        self.modified_blocks.insert((wx, wy, wz), block);
        self.save_dirty = true;
        true
    }

    pub fn is_solid_at_world(&self, wx: i32, wy: i32, wz: i32) -> bool {
        if wy < 0 {
            return true;
        }
        if wy >= super::chunk::CHUNK_H as i32 {
            return false;
        }

        let cx = wx.div_euclid(CHUNK_W as i32);
        let cz = wz.div_euclid(CHUNK_D as i32);
        let lx = wx.rem_euclid(CHUNK_W as i32) as usize;
        let lz = wz.rem_euclid(CHUNK_D as i32) as usize;

        if let Some(chunk) = self.chunks.get(&(cx, cz)) {
            return chunk.get(lx, wy as usize, lz).is_solid();
        }

        false
    }

    fn queue_chunk_generation(&mut self, cx: i32, cz: i32) {
        if self.in_flight_gen.len() >= MAX_IN_FLIGHT_GEN_JOBS {
            return;
        }
        let mut sent_gen = 0;
        let rd = self.render_dist;
        'gen: for r in 0..=(rd + 1) {
            for dx in -r..=r {
                for dz in -r..=r {
                    if dx.abs() != r && dz.abs() != r {
                        continue;
                    }

                    let key = (cx + dx, cz + dz);
                    if self.chunks.contains_key(&key) || self.in_flight_gen.contains(&key) {
                        continue;
                    }

                    if self
                        .gen_tx
                        .try_send((key.0, key.1, self.seed, self.world_type))
                        .is_ok()
                    {
                        self.in_flight_gen.insert(key);
                        sent_gen += 1;
                        if sent_gen >= 4 {
                            break 'gen;
                        }
                    }
                }
            }
        }
    }

    fn receive_generated_chunks(&mut self) {
        for _ in 0..MAX_GEN_RESULTS_PER_UPDATE {
            let Ok((key, chunk)) = self.gen_rx.try_recv() else {
                break;
            };
            self.in_flight_gen.remove(&key);
            self.chunks.insert(key, Arc::new(chunk));
            self.apply_pending_mods_for_chunk(key);
            self.mark_chunk_and_neighbors_dirty(key);
        }
    }

    fn queue_dirty_meshes(&mut self) {
        if self.in_flight_mesh.len() >= MAX_IN_FLIGHT_MESH_JOBS {
            return;
        }
        let dirty = std::mem::take(&mut self.dirty_mesh);
        let mut queued = 0usize;
        for key in dirty {
            if queued >= MAX_MESH_TASKS_PER_UPDATE {
                self.dirty_mesh.insert(key);
                continue;
            }
            if self.in_flight_mesh.contains(&key) {
                // Keep the key dirty to mesh again after current in-flight job completes.
                self.dirty_mesh.insert(key);
                continue;
            }
            let Some(chunk) = self.chunks.get(&key) else {
                continue;
            };

            let task = MeshTask {
                key,
                chunk: Arc::clone(chunk),
                px: self.chunks.get(&(key.0 - 1, key.1)).map(Arc::clone),
                nx: self.chunks.get(&(key.0 + 1, key.1)).map(Arc::clone),
                pz: self.chunks.get(&(key.0, key.1 - 1)).map(Arc::clone),
                nz: self.chunks.get(&(key.0, key.1 + 1)).map(Arc::clone),
            };

            if self.mesh_tx.try_send(task).is_ok() {
                self.in_flight_mesh.insert(key);
                queued += 1;
            } else {
                // Channel is full: keep dirty for the next frame.
                self.dirty_mesh.insert(key);
            }
        }
    }

    fn receive_ready_meshes(&mut self) {
        for _ in 0..MAX_MESH_RESULTS_PER_UPDATE {
            let Ok((key, mesh)) = self.mesh_rx.try_recv() else {
                break;
            };
            self.in_flight_mesh.remove(&key);

            // Drop stale mesh outputs for chunks that were unloaded or already re-marked dirty.
            if !self.chunks.contains_key(&key) || self.dirty_mesh.contains(&key) {
                continue;
            }
            self.ready_meshes.push((key, mesh));
        }
    }

    fn unload_far_chunks(&mut self, cx: i32, cz: i32) {
        let rd = self.render_dist;
        let unload: Vec<_> = self
            .chunks
            .keys()
            .filter(|(x, z)| (x - cx).abs() > rd + 2 || (z - cz).abs() > rd + 2)
            .cloned()
            .collect();

        for key in &unload {
            self.chunks.remove(key);
            self.dirty_mesh.remove(key);
            self.in_flight_gen.remove(key);
            self.in_flight_mesh.remove(key);
        }
        self.removed.extend_from_slice(&unload);
    }

    fn load_saved_blocks(&mut self) {
        self.modified_blocks = save::load_world_blocks(self.seed);
        let entries: Vec<_> = self
            .modified_blocks
            .iter()
            .map(|(&(x, y, z), &block)| (x, y, z, block))
            .collect();
        for (wx, wy, wz, block) in entries {
            if wy < 0 || wy >= super::chunk::CHUNK_H as i32 {
                continue;
            }
            let cx = wx.div_euclid(CHUNK_W as i32);
            let cz = wz.div_euclid(CHUNK_D as i32);
            self.upsert_pending_block_mod((cx, cz), wx, wy, wz, block);
        }
        if !self.modified_blocks.is_empty() {
            log::info!(
                "Loaded {} saved block modifications for seed {}",
                self.modified_blocks.len(),
                self.seed
            );
        }
    }

    fn upsert_pending_block_mod(
        &mut self,
        key: (i32, i32),
        wx: i32,
        wy: i32,
        wz: i32,
        block: Block,
    ) {
        let mods = self.pending_block_mods.entry(key).or_default();
        if let Some(existing) = mods
            .iter_mut()
            .find(|(x, y, z, _)| *x == wx && *y == wy && *z == wz)
        {
            existing.3 = block;
        } else {
            mods.push((wx, wy, wz, block));
        }
    }

    fn apply_pending_mods_for_chunk(&mut self, key: (i32, i32)) {
        let Some(mods) = self.pending_block_mods.get(&key) else {
            return;
        };
        let Some(chunk_arc) = self.chunks.get(&key).cloned() else {
            return;
        };

        let mut blocks = chunk_arc.blocks.to_vec().into_boxed_slice();
        let mut changed = false;
        for &(wx, wy, wz, block) in mods {
            if wy < 0 || wy >= super::chunk::CHUNK_H as i32 {
                continue;
            }
            let lx = wx.rem_euclid(CHUNK_W as i32) as usize;
            let lz = wz.rem_euclid(CHUNK_D as i32) as usize;
            let i = idx(lx, wy as usize, lz);
            if blocks[i] != block {
                blocks[i] = block;
                changed = true;
            }
        }

        if changed {
            let new_chunk = Chunk {
                cx: chunk_arc.cx,
                cz: chunk_arc.cz,
                blocks: blocks.into(),
                heightmap: Arc::clone(&chunk_arc.heightmap),
            };
            self.chunks.insert(key, Arc::new(new_chunk));
        }
    }

    fn mark_chunk_and_neighbors_dirty(&mut self, key: (i32, i32)) {
        self.dirty_mesh.insert(key);
        for &(dx, dz) in &CARDINAL_NEIGHBORS {
            let nb = (key.0 + dx, key.1 + dz);
            if self.chunks.contains_key(&nb) {
                self.dirty_mesh.insert(nb);
            }
        }
    }
}
