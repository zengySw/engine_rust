use std::path::PathBuf;

/// Command line args.
/// Example: `engine.exe -threads 8 -render_dist 6 -vsync --world saves/world_42.rc`
#[derive(Debug, Clone)]
pub struct Args {
    pub threads: usize,
    pub render_dist: i32,
    pub vsync: bool,
    pub world_file: Option<PathBuf>,
    pub import_jar: Option<PathBuf>,
}

impl Args {
    pub fn parse() -> Self {
        let args: Vec<String> = std::env::args().collect();
        let mut threads = num_cpus();
        let mut render_dist = 6;
        let mut vsync = false;
        let mut world_file: Option<PathBuf> = None;
        let mut import_jar: Option<PathBuf> = None;

        let mut i = 1;
        while i < args.len() {
            match args[i].as_str() {
                "-threads" | "--threads" => {
                    if let Some(v) = args.get(i + 1).and_then(|s| s.parse().ok()) {
                        threads = v;
                        i += 1;
                    }
                }
                "-render_dist" | "--render_dist" => {
                    if let Some(v) = args.get(i + 1).and_then(|s| s.parse().ok()) {
                        render_dist = v;
                        i += 1;
                    }
                }
                "-vsync" | "--vsync" => vsync = true,
                "-world" | "--world" => {
                    if let Some(v) = args.get(i + 1) {
                        world_file = Some(PathBuf::from(v));
                        i += 1;
                    }
                }
                "-import-jar" | "--import-jar" | "-import-mod" | "--import-mod" => {
                    if let Some(v) = args.get(i + 1) {
                        import_jar = Some(PathBuf::from(v));
                        i += 1;
                    }
                }
                other => {
                    // If app is launched from an associated .rc file, it usually arrives as a positional arg.
                    if world_file.is_none() && !other.starts_with('-') {
                        let lower = other.to_ascii_lowercase();
                        if lower.ends_with(".rc") {
                            world_file = Some(PathBuf::from(other));
                        }
                    }
                }
            }
            i += 1;
        }

        let threads = threads.clamp(1, 32);
        log::info!(
            "Config: threads={} render_dist={} vsync={} world_file={}",
            threads,
            render_dist,
            vsync,
            world_file
                .as_ref()
                .and_then(|p| p.to_str())
                .unwrap_or("<default>"),
        );
        if let Some(path) = import_jar.as_ref() {
            log::info!(
                "Mod import requested from {}",
                path.to_str().unwrap_or("<invalid path>")
            );
        }

        Self {
            threads,
            render_dist,
            vsync,
            world_file,
            import_jar,
        }
    }

    /// How many threads each pool gets.
    /// threads=8 -> gen=2, mesh=4, hmap=2
    pub fn hmap_threads(&self) -> usize {
        (self.threads / 4).max(1)
    }
    pub fn gen_threads(&self) -> usize {
        (self.threads / 4).max(1)
    }
    pub fn mesh_threads(&self) -> usize {
        (self.threads / 2).max(1)
    }
    pub fn cull_threads(&self) -> usize {
        (self.threads / 4).max(1)
    }
}

fn num_cpus() -> usize {
    std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(4)
}
