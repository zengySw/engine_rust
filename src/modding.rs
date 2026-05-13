use std::sync::{
    mpsc::{self, Receiver, Sender},
    Arc, OnceLock, RwLock,
};
use std::time::Duration;

use crate::world::biome::Biome;
use crate::world::block::Block;

#[allow(dead_code)]
#[derive(Clone, Debug)]
pub struct ApiSnapshot {
    pub world_seed: u32,
    pub day_time: f32,
    pub player_feet: [f32; 3],
    pub player_eye: [f32; 3],
    pub chunks_loaded: usize,
    pub debug_overlay: bool,
    pub menu_open: bool,
    pub inventory_open: bool,
    pub ray_tracing_enabled: bool,
}

impl Default for ApiSnapshot {
    fn default() -> Self {
        Self {
            world_seed: 0,
            day_time: 0.25,
            player_feet: [0.0, 0.0, 0.0],
            player_eye: [0.0, 0.0, 0.0],
            chunks_loaded: 0,
            debug_overlay: false,
            menu_open: false,
            inventory_open: false,
            ray_tracing_enabled: false,
        }
    }
}

#[allow(dead_code)]
pub enum ApiCommand {
    RegenerateWorld { seed: Option<u32> },
    SetTimeOfDay(f32),
    AddTimeOfDay(f32),
    SetPlayerPosition { x: f32, y: f32, z: f32 },
    SetDebugOverlay(bool),
    SetMenuOpen(bool),
    SetInventoryOpen(bool),
    SetMouseSensitivity(f32),
    SetMoveSpeed(f32),
    SetRayTracingEnabled(bool),
    SetBlock {
        x: i32,
        y: i32,
        z: i32,
        block: Block,
    },
    QueryBlock {
        x: i32,
        y: i32,
        z: i32,
        respond_to: Sender<Block>,
    },
    QueryBiome {
        x: i32,
        z: i32,
        respond_to: Sender<Option<Biome>>,
    },
    QuerySurfaceY {
        x: i32,
        z: i32,
        respond_to: Sender<Option<u32>>,
    },
}

#[derive(Clone)]
#[allow(dead_code)]
pub struct ModApi {
    tx: Sender<ApiCommand>,
    snapshot: Arc<RwLock<ApiSnapshot>>,
}

pub struct ModApiRuntime {
    rx: Receiver<ApiCommand>,
    snapshot: Arc<RwLock<ApiSnapshot>>,
}

static GLOBAL_API: OnceLock<ModApi> = OnceLock::new();

pub fn create_api(initial_seed: u32) -> (ModApi, ModApiRuntime) {
    let (tx, rx) = mpsc::channel();
    let snapshot = Arc::new(RwLock::new(ApiSnapshot {
        world_seed: initial_seed,
        ..ApiSnapshot::default()
    }));
    let api = ModApi {
        tx,
        snapshot: Arc::clone(&snapshot),
    };
    let runtime = ModApiRuntime { rx, snapshot };
    (api, runtime)
}

pub fn install_global_api(api: ModApi) {
    let _ = GLOBAL_API.set(api);
}

#[allow(dead_code)]
pub fn global_api() -> Option<&'static ModApi> {
    GLOBAL_API.get()
}

impl ModApi {
    #[allow(dead_code)]
    pub fn send(&self, cmd: ApiCommand) -> Result<(), mpsc::SendError<ApiCommand>> {
        self.tx.send(cmd)
    }

    #[allow(dead_code)]
    pub fn snapshot(&self) -> ApiSnapshot {
        self.snapshot
            .read()
            .map(|s| s.clone())
            .unwrap_or_default()
    }

    #[allow(dead_code)]
    pub fn regenerate_world(&self, seed: Option<u32>) {
        let _ = self.send(ApiCommand::RegenerateWorld { seed });
    }

    #[allow(dead_code)]
    pub fn set_time_of_day(&self, day_time: f32) {
        let _ = self.send(ApiCommand::SetTimeOfDay(day_time));
    }

    #[allow(dead_code)]
    pub fn set_player_position(&self, x: f32, y: f32, z: f32) {
        let _ = self.send(ApiCommand::SetPlayerPosition { x, y, z });
    }

    #[allow(dead_code)]
    pub fn set_ray_tracing_enabled(&self, enabled: bool) {
        let _ = self.send(ApiCommand::SetRayTracingEnabled(enabled));
    }

    #[allow(dead_code)]
    pub fn set_block(&self, x: i32, y: i32, z: i32, block: Block) {
        let _ = self.send(ApiCommand::SetBlock { x, y, z, block });
    }

    #[allow(dead_code)]
    pub fn query_block(&self, x: i32, y: i32, z: i32) -> Option<Block> {
        let (tx, rx) = mpsc::channel();
        if self
            .send(ApiCommand::QueryBlock {
                x,
                y,
                z,
                respond_to: tx,
            })
            .is_err()
        {
            return None;
        }
        rx.recv_timeout(Duration::from_millis(80)).ok()
    }

    #[allow(dead_code)]
    pub fn query_biome(&self, x: i32, z: i32) -> Option<Biome> {
        let (tx, rx) = mpsc::channel();
        if self
            .send(ApiCommand::QueryBiome {
                x,
                z,
                respond_to: tx,
            })
            .is_err()
        {
            return None;
        }
        rx.recv_timeout(Duration::from_millis(80)).ok().flatten()
    }
}

impl ModApiRuntime {
    pub fn try_recv(&mut self) -> Option<ApiCommand> {
        self.rx.try_recv().ok()
    }

    pub fn update_snapshot(&self, snapshot: ApiSnapshot) {
        if let Ok(mut s) = self.snapshot.write() {
            *s = snapshot;
        }
    }
}
