use std::path::PathBuf;

fn push_unique(paths: &mut Vec<PathBuf>, path: PathBuf) {
    if !paths.iter().any(|p| p == &path) {
        paths.push(path);
    }
}

pub fn base_roots() -> Vec<PathBuf> {
    let mut roots = Vec::new();

    if let Ok(cwd) = std::env::current_dir() {
        push_unique(&mut roots, cwd);
    }

    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            push_unique(&mut roots, dir.to_path_buf());
        }
    }

    push_unique(&mut roots, PathBuf::from(env!("CARGO_MANIFEST_DIR")));
    roots
}

pub fn asset_roots() -> Vec<PathBuf> {
    let mut roots = Vec::new();
    for base in base_roots() {
        push_unique(&mut roots, base.join("assets"));
        push_unique(&mut roots, base.join("src").join("assets"));
    }
    roots
}

pub fn saves_dir() -> PathBuf {
    if let Ok(custom) = std::env::var("RUSTYCRAFT_SAVES_DIR") {
        let custom = custom.trim();
        if !custom.is_empty() {
            return PathBuf::from(custom);
        }
    }

    if let Some(base) = base_roots().into_iter().next() {
        return base.join("saves");
    }

    PathBuf::from("saves")
}

