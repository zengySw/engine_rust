use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};
use std::sync::mpsc::{self, Receiver, SyncSender};
use super::chunk::{Chunk, Heightmap, Vertex, CHUNK_W, CHUNK_D};
use crate::args::Args;

pub struct ChunkMesh { pub verts: Vec<Vertex> }

struct MeshTask {
    key:   (i32,i32),
    chunk: Arc<Chunk>,
    px: Option<Arc<Chunk>>,
    nx: Option<Arc<Chunk>>,
    pz: Option<Arc<Chunk>>,
    nz: Option<Arc<Chunk>>,
}

pub struct World {
    pub chunks:         HashMap<(i32,i32), Arc<Chunk>>,
    heightmaps:         HashMap<(i32,i32), Heightmap>,
    seed:               u32,
    render_dist:        i32,

    hmap_tx:            SyncSender<(i32,i32,u32)>,
    hmap_rx:            Receiver<((i32,i32), Heightmap)>,
    in_flight_hmap:     HashSet<(i32,i32)>,

    gen_tx:             SyncSender<(i32,i32,u32,Heightmap)>,
    gen_rx:             Receiver<((i32,i32), Chunk)>,
    in_flight_gen:      HashSet<(i32,i32)>,

    mesh_tx:            SyncSender<MeshTask>,
    mesh_rx:            Receiver<((i32,i32), ChunkMesh)>,
    in_flight_mesh:     HashSet<(i32,i32)>,
    dirty_mesh:         HashSet<(i32,i32)>,

    pub ready_meshes:   Vec<((i32,i32), ChunkMesh)>,
    pub removed:        Vec<(i32,i32)>,
}

impl World {
    pub fn new(seed: u32, args: &Args) -> Self {
        let render_dist = args.render_dist;

        // ── Пул heightmap ─────────────────────────────────────
        let (hmap_tx, hmap_task_rx) = mpsc::sync_channel::<(i32,i32,u32)>(64);
        let (hmap_result_tx, hmap_rx) = mpsc::sync_channel::<((i32,i32), Heightmap)>(64);
        let hmap_pool = rayon::ThreadPoolBuilder::new()
            .num_threads(args.hmap_threads())
            .thread_name(|i| format!("hmap-{i}"))
            .build().unwrap();
        let hmap_tx2 = Arc::new(Mutex::new(hmap_result_tx));

        std::thread::spawn(move || {
            while let Ok((cx, cz, seed)) = hmap_task_rx.recv() {
                let tx = Arc::clone(&hmap_tx2);
                hmap_pool.spawn(move || {
                    let h = Heightmap::generate(cx, cz, seed);
                    let _ = tx.lock().unwrap().send(((cx,cz), h));
                });
            }
        });

        // ── Пул генерации блоков ──────────────────────────────
        let (gen_tx, gen_task_rx) = mpsc::sync_channel::<(i32,i32,u32,Heightmap)>(32);
        let (gen_result_tx, gen_rx) = mpsc::sync_channel::<((i32,i32), Chunk)>(32);
        let gen_pool = rayon::ThreadPoolBuilder::new()
            .num_threads(args.gen_threads())
            .thread_name(|i| format!("gen-{i}"))
            .build().unwrap();
        let gen_tx2 = Arc::new(Mutex::new(gen_result_tx));

        std::thread::spawn(move || {
            while let Ok((cx, cz, seed, hmap)) = gen_task_rx.recv() {
                let tx = Arc::clone(&gen_tx2);
                gen_pool.spawn(move || {
                    let chunk = Chunk::generate(cx, cz, seed, hmap, [None,None,None,None]);
                    let _ = tx.lock().unwrap().send(((cx,cz), chunk));
                });
            }
        });

        // ── Пул меша ─────────────────────────────────────────
        let (mesh_tx, mesh_task_rx) = mpsc::sync_channel::<MeshTask>(64);
        let (mesh_result_tx, mesh_rx) = mpsc::sync_channel::<((i32,i32), ChunkMesh)>(64);
        let mesh_pool = rayon::ThreadPoolBuilder::new()
            .num_threads(args.mesh_threads())
            .thread_name(|i| format!("mesh-{i}"))
            .build().unwrap();
        let mesh_tx2 = Arc::new(Mutex::new(mesh_result_tx));

        std::thread::spawn(move || {
            while let Ok(task) = mesh_task_rx.recv() {
                let tx = Arc::clone(&mesh_tx2);
                mesh_pool.spawn(move || {
                    let verts = task.chunk.build_mesh(
                        task.px.as_deref(), task.nx.as_deref(),
                        task.pz.as_deref(), task.nz.as_deref(),
                    );
                    let _ = tx.lock().unwrap().send((task.key, ChunkMesh { verts }));
                });
            }
        });

        Self {
            chunks: HashMap::new(), heightmaps: HashMap::new(),
            seed, render_dist,
            hmap_tx, hmap_rx, in_flight_hmap: HashSet::new(),
            gen_tx, gen_rx, in_flight_gen: HashSet::new(),
            mesh_tx, mesh_rx,
            in_flight_mesh: HashSet::new(),
            dirty_mesh: HashSet::new(),
            ready_meshes: Vec::new(), removed: Vec::new(),
        }
    }

    pub fn update(&mut self, player_x: f32, player_z: f32) {
        let cx = (player_x / CHUNK_W as f32).floor() as i32;
        let cz = (player_z / CHUNK_D as f32).floor() as i32;
        let rd = self.render_dist;

        // ── 1. Запрашиваем heightmap'ы ────────────────────────
        let mut sent = 0;
        'hmap: for r in 0..=(rd+1) {
            for dx in -r..=r { for dz in -r..=r {
                if dx.abs() != r && dz.abs() != r { continue; }
                let key = (cx+dx, cz+dz);
                if !self.heightmaps.contains_key(&key) && !self.in_flight_hmap.contains(&key) {
                    self.in_flight_hmap.insert(key);
                    let _ = self.hmap_tx.try_send((key.0, key.1, self.seed));
                    sent += 1;
                    if sent >= 8 { break 'hmap; }
                }
            }}
        }

        // ── 2. Принимаем готовые heightmap'ы ─────────────────
        while let Ok((key, hmap)) = self.hmap_rx.try_recv() {
            self.in_flight_hmap.remove(&key);
            self.heightmaps.insert(key, hmap);
        }

        // ── 3. Отправляем задачи генерации блоков ────────────
        let mut sent_gen = 0;
        'gen: for r in 0..=rd {
            for dx in -r..=r { for dz in -r..=r {
                if dx.abs() != r && dz.abs() != r { continue; }
                let key = (cx+dx, cz+dz);
                if self.chunks.contains_key(&key) || self.in_flight_gen.contains(&key) { continue; }

                let Some(raw_hmap) = self.heightmaps.get(&key) else { continue };
                let px = self.heightmaps.get(&(key.0-1, key.1));
                let nx = self.heightmaps.get(&(key.0+1, key.1));
                let pz = self.heightmaps.get(&(key.0, key.1-1));
                let nz = self.heightmaps.get(&(key.0, key.1+1));

                let mut blended_surface = raw_hmap.surface.clone();
                for x in 0..CHUNK_W {
                    for z in 0..CHUNK_D {
                        blended_surface[x][z] = raw_hmap.blended_surface(x, z, [px, nx, pz, nz]);
                    }
                }
                let blended = Heightmap { surface: blended_surface, biome: raw_hmap.biome.clone() };

                self.in_flight_gen.insert(key);
                let _ = self.gen_tx.try_send((key.0, key.1, self.seed, blended));
                sent_gen += 1;
                if sent_gen >= 2 { break 'gen; }
            }}
        }

        // ── 4. Принимаем сгенерированные чанки ───────────────
        while let Ok((key, chunk)) = self.gen_rx.try_recv() {
            self.in_flight_gen.remove(&key);
            self.chunks.insert(key, Arc::new(chunk));
            self.dirty_mesh.insert(key);
            for &(dx,dz) in &[(1,0),(-1,0),(0,1),(0,-1)] {
                let nb = (key.0+dx, key.1+dz);
                if self.chunks.contains_key(&nb) { self.dirty_mesh.insert(nb); }
            }
        }

        // ── 5. Отправляем dirty на меш ────────────────────────
        let dirty: Vec<_> = self.dirty_mesh.drain().collect();
        for key in dirty {
            if self.in_flight_mesh.contains(&key) { continue; }
            let Some(chunk) = self.chunks.get(&key) else { continue };
            let task = MeshTask {
                key,
                chunk: Arc::clone(chunk),
                px: self.chunks.get(&(key.0-1, key.1)).map(Arc::clone),
                nx: self.chunks.get(&(key.0+1, key.1)).map(Arc::clone),
                pz: self.chunks.get(&(key.0, key.1-1)).map(Arc::clone),
                nz: self.chunks.get(&(key.0, key.1+1)).map(Arc::clone),
            };
            if self.mesh_tx.try_send(task).is_ok() {
                self.in_flight_mesh.insert(key);
            }
        }

        // ── 6. Забираем готовые меши ──────────────────────────
        while let Ok(mesh) = self.mesh_rx.try_recv() {
            self.in_flight_mesh.remove(&mesh.0);
            self.ready_meshes.push(mesh);
        }

        // ── 7. Выгрузка ───────────────────────────────────────
        let unload: Vec<_> = self.chunks.keys()
            .filter(|(x,z)| (x-cx).abs() > rd+2 || (z-cz).abs() > rd+2)
            .cloned().collect();
        for key in &unload {
            self.chunks.remove(key);
            self.heightmaps.remove(key);
            self.dirty_mesh.remove(key);
        }
        self.removed.extend_from_slice(&unload);
    }

    pub fn chunk_count(&self) -> usize { self.chunks.len() }

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
}
