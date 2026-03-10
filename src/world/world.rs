use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};
use std::sync::mpsc::{self, Receiver, SyncSender};
use super::chunk::{Chunk, Heightmap, Vertex, CHUNK_W, CHUNK_D};

const RENDER_DIST: i32   = 5;   // было 6 — меньше чанков сразу
const MAX_GEN_PER_FRAME: usize  = 2; // heightmap за кадр (не блокируем)
const MAX_MESH_PER_FRAME: usize = 6; // мешей на GPU за кадр

pub struct ChunkMesh { pub verts: Vec<Vertex> }

// Простые HashMap без Arc — читаем/пишем только с главного потока
// Фоновые потоки получают клон данных через канал
pub struct World {
    // Главный поток владеет этим напрямую
    pub chunks:    HashMap<(i32,i32), Arc<Chunk>>,
    heightmaps:    HashMap<(i32,i32), Heightmap>,
    seed:          u32,

    // Очереди задач (bounded — не переполняются)
    hmap_tx:       SyncSender<(i32,i32,u32)>,        // задача: cx,cz,seed
    hmap_rx:       Receiver<((i32,i32), Heightmap)>, // готовые heightmap'ы
    in_flight_hmap:HashSet<(i32,i32)>,

    gen_tx:        SyncSender<(i32, i32, u32, Heightmap)>,
    gen_rx:        Receiver<((i32,i32), Chunk)>,    in_flight_gen: HashSet<(i32,i32)>,

    mesh_tx:       SyncSender<MeshTask>,
    mesh_rx:       Receiver<((i32,i32), ChunkMesh)>,    in_flight_mesh:HashSet<(i32,i32)>,
    dirty_mesh:    HashSet<(i32,i32)>,

    pub ready_meshes: Vec<((i32,i32), ChunkMesh)>,
    pub removed:      Vec<(i32,i32)>,
}

struct MeshTask {
    key:   (i32,i32),
    chunk: Arc<Chunk>, // Arc — не копируем данные
    px: Option<Arc<Chunk>>,
    nx: Option<Arc<Chunk>>,
    pz: Option<Arc<Chunk>>,
    nz: Option<Arc<Chunk>>,
}

impl World {
    pub fn new(seed: u32) -> Self {
        // ── Пул для heightmap ─────────────────────────────────
        let (hmap_tx, hmap_task_rx) = mpsc::sync_channel::<(i32,i32,u32)>(64);
        let (hmap_result_tx, hmap_rx) = mpsc::sync_channel::<((i32,i32), Heightmap)>(64);
        let hmap_pool = rayon::ThreadPoolBuilder::new().num_threads(2).build().unwrap();
        let hmap_result_tx = Arc::new(Mutex::new(hmap_result_tx));

        std::thread::spawn(move || {
            while let Ok((cx, cz, seed)) = hmap_task_rx.recv() {
                let tx = Arc::clone(&hmap_result_tx);
                hmap_pool.spawn(move || {
                    let h = Heightmap::generate(cx, cz, seed);
                    let _ = tx.lock().unwrap().send(((cx,cz), h));
                });
            }
        });

        // ── Пул для генерации блоков ──────────────────────────
        let (gen_tx, gen_task_rx) = mpsc::sync_channel::<(i32,i32,u32,Heightmap)>(32);
        let (gen_result_tx, gen_rx) = mpsc::sync_channel::<((i32,i32), Chunk)>(32);
        let gen_pool = rayon::ThreadPoolBuilder::new().num_threads(4).build().unwrap();
        let gen_result_tx = Arc::new(Mutex::new(gen_result_tx));

        std::thread::spawn(move || {
            while let Ok((cx, cz, seed, hmap)) = gen_task_rx.recv() {
                let tx = Arc::clone(&gen_result_tx);
                gen_pool.spawn(move || {
                    let chunk = Chunk::generate(cx, cz, seed, hmap, [None,None,None,None]);
                    let _ = tx.lock().unwrap().send(((cx,cz), chunk));
                });
            }
        });

        // ── Пул для меша ─────────────────────────────────────
        let (mesh_tx, mesh_task_rx) = mpsc::sync_channel::<MeshTask>(64);
        let (mesh_result_tx, mesh_rx) = mpsc::sync_channel::<((i32,i32), ChunkMesh)>(64);
        let mesh_pool = rayon::ThreadPoolBuilder::new().num_threads(4).build().unwrap();
        let mesh_result_tx = Arc::new(Mutex::new(mesh_result_tx));

        std::thread::spawn(move || {
            while let Ok(task) = mesh_task_rx.recv() {
                let tx = Arc::clone(&mesh_result_tx);
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
            chunks: HashMap::new(), heightmaps: HashMap::new(), seed,
            render_dist,
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

        // ── 1. Запрашиваем heightmap'ы ────────────────────────
        // Лимитируем чтобы не спамить задачами
        let mut sent_hmap = 0;
        'outer_h: for r in 0..=(RENDER_DIST+1) {
            for dx in -r..=r {
                for dz in -r..=r {
                    if dx.abs() != r && dz.abs() != r { continue; }
                    let key = (cx+dx, cz+dz);
                    if !self.heightmaps.contains_key(&key)
                        && !self.in_flight_hmap.contains(&key)
                    {
                        self.in_flight_hmap.insert(key);
                        let _ = self.hmap_tx.try_send((key.0, key.1, self.seed));
                        sent_hmap += 1;
                        if sent_hmap >= MAX_GEN_PER_FRAME * 4 { break 'outer_h; }
                    }
                }
            }
        }

        // ── 2. Принимаем готовые heightmap'ы ─────────────────
        while let Ok((key, hmap)) = self.hmap_rx.try_recv() {
            self.in_flight_hmap.remove(&key);
            self.heightmaps.insert(key, hmap);
        }

        // ── 3. Отправляем задачи генерации блоков ────────────
        // Спираль от центра — ближние чанки грузятся первыми
        let mut sent_gen = 0;
        'outer_g: for r in 0..=RENDER_DIST {
            for dx in -r..=r {
                for dz in -r..=r {
                    if dx.abs() != r && dz.abs() != r { continue; }
                    let key = (cx+dx, cz+dz);
                    if self.chunks.contains_key(&key)
                        || self.in_flight_gen.contains(&key) { continue; }

                    let Some(raw_hmap) = self.heightmaps.get(&key) else { continue };

                    // Блендим heightmap с соседями
                    let px = self.heightmaps.get(&(key.0-1, key.1));
                    let nx = self.heightmaps.get(&(key.0+1, key.1));
                    let pz = self.heightmaps.get(&(key.0, key.1-1));
                    let nz = self.heightmaps.get(&(key.0, key.1+1));

                    let mut blended_surface = raw_hmap.surface.clone();
                    for x in 0..CHUNK_W {
                        for z in 0..CHUNK_D {
                            blended_surface[x][z] =
                                raw_hmap.blended_surface(x, z, [px, nx, pz, nz]);
                        }
                    }
                    // Новый heightmap с заблендированной поверхностью
                    let blended = Heightmap {
                        surface: blended_surface,
                        biome:   raw_hmap.biome.clone(),
                    };

                    self.in_flight_gen.insert(key);
                    let _ = self.gen_tx.try_send((key.0, key.1, self.seed, blended));
                    sent_gen += 1;
                    if sent_gen >= MAX_GEN_PER_FRAME { break 'outer_g; }
                }
            }
        }

        // ── 4. Принимаем сгенерированные чанки ───────────────
        while let Ok((key, chunk)) = self.gen_rx.try_recv() {
            self.in_flight_gen.remove(&key);
            self.chunks.insert(key, Arc::new(chunk));
            self.dirty_mesh.insert(key);
            for &(dx,dz) in &[(1,0),(-1,0),(0,1),(0,-1)] {
                let nb = (key.0+dx, key.1+dz);
                if self.chunks.contains_key(&nb) {
                    self.dirty_mesh.insert(nb);
                }
            }
        }

        // ── 5. Отправляем dirty чанки на меш ─────────────────
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

        // ── 6. Принимаем готовые меши ─────────────────────────
        while let Ok(mesh) = self.mesh_rx.try_recv() {
            self.in_flight_mesh.remove(&mesh.0);
            self.ready_meshes.push(mesh);
        }

        // ── 7. Выгрузка ───────────────────────────────────────
        let unload: Vec<_> = self.chunks.keys()
            .filter(|(x,z)| (x-cx).abs() > RENDER_DIST+2 || (z-cz).abs() > RENDER_DIST+2)
            .cloned().collect();
        for key in &unload {
            self.chunks.remove(key);
            self.heightmaps.remove(key);
            self.dirty_mesh.remove(key);
        }
        self.removed.extend_from_slice(&unload);
    }

    pub fn chunk_count(&self) -> usize { self.chunks.len() }
}