/// Аргументы командной строки
/// Использование: engine.exe -threads 8 -render_dist 6 -vsync
#[derive(Debug, Clone)]
pub struct Args {
    pub threads:     usize, // кол-во потоков для всех пулов
    pub render_dist: i32,   // дистанция рендера в чанках
    pub vsync:       bool,
}

impl Args {
    pub fn parse() -> Self {
        let args: Vec<String> = std::env::args().collect();
        let mut threads     = num_cpus();
        let mut render_dist = 6;
        let mut vsync       = false;

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
                _ => {}
            }
            i += 1;
        }

        let threads = threads.clamp(1, 32);
        log::info!(
            "Config: threads={} render_dist={} vsync={}",
            threads, render_dist, vsync
        );

        Self { threads, render_dist, vsync }
    }

    /// Сколько потоков отдать каждому пулу
    /// threads=8 → gen=2, mesh=4, hmap=2
    pub fn hmap_threads(&self)  -> usize { (self.threads / 4).max(1) }
    pub fn gen_threads(&self)   -> usize { (self.threads / 4).max(1) }
    pub fn mesh_threads(&self)  -> usize { (self.threads / 2).max(1) }
    pub fn cull_threads(&self)  -> usize { (self.threads / 4).max(1) }
}

fn num_cpus() -> usize {
    // std не имеет num_cpus, используем простой способ
    std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(4)
}