use std::collections::HashMap;
use std::fs::{self, File};
use std::io::BufReader;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use rodio::{Decoder, OutputStream, OutputStreamHandle, Sink};

use crate::world::block::Block;

pub struct SoundSystem {
    _stream: OutputStream,
    handle: OutputStreamHandle,
    break_sounds: HashMap<String, Vec<PathBuf>>,
    walk_sounds: HashMap<String, Vec<PathBuf>>,
    misc_sounds: HashMap<String, Vec<PathBuf>>,
    break_cursors: HashMap<String, usize>,
    walk_cursors: HashMap<String, usize>,
    misc_cursors: HashMap<String, usize>,
    rng_state: u32,
    master_volume: f32,
    music_volume: f32,
    ambient_volume: f32,
    sfx_volume: f32,
}

#[derive(Clone, Copy)]
enum SoundBus {
    Music,
    Ambient,
    Sfx,
}

impl SoundSystem {
    pub fn new() -> Option<Self> {
        let (stream, handle) = match OutputStream::try_default() {
            Ok(v) => v,
            Err(err) => {
                log::warn!("Audio output unavailable: {err}");
                return None;
            }
        };

        let mut break_sounds = load_sound_map("_break");
        for sounds in break_sounds.values_mut() {
            sounds.sort();
        }
        let mut walk_sounds = load_sound_map("_walk");
        for sounds in walk_sounds.values_mut() {
            sounds.sort();
        }
        let mut misc_sounds = load_misc_sound_map();
        for sounds in misc_sounds.values_mut() {
            sounds.sort();
        }
        if break_sounds.is_empty() {
            log::warn!("No break sounds found (expected *_break.mp3 or block/<family>/break/*.mp3)");
        }
        if walk_sounds.is_empty() {
            log::warn!("No walk sounds found (expected *_walk.mp3 or block/<family>/walk/*.mp3)");
        }

        let mut rng_state = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.subsec_nanos())
            .unwrap_or(0xA5A5_1234);
        if rng_state == 0 {
            rng_state = 0xA5A5_1234;
        }

        Some(Self {
            _stream: stream,
            handle,
            break_sounds,
            walk_sounds,
            misc_sounds,
            break_cursors: HashMap::new(),
            walk_cursors: HashMap::new(),
            misc_cursors: HashMap::new(),
            rng_state,
            master_volume: 1.0,
            music_volume: 1.0,
            ambient_volume: 1.0,
            sfx_volume: 1.0,
        })
    }

    pub fn set_mix(&mut self, master: f32, music: f32, ambient: f32, sfx: f32) {
        self.master_volume = master.clamp(0.0, 1.0);
        self.music_volume = music.clamp(0.0, 1.0);
        self.ambient_volume = ambient.clamp(0.0, 1.0);
        self.sfx_volume = sfx.clamp(0.0, 1.0);
    }

    pub fn play_block_break(&mut self, block: Block) {
        let Some((family, path)) = pick_sound_from_map(
            &self.break_sounds,
            &mut self.break_cursors,
            break_sound_families(block),
        ) else {
            return;
        };
        self.play_from_file(path, self.mix_volume(0.55, SoundBus::Sfx), &family);
    }

    pub fn play_footstep(&mut self, block: Block) {
        let Some((family, path)) = pick_sound_from_map(
            &self.walk_sounds,
            &mut self.walk_cursors,
            walk_sound_families(block),
        ) else {
            return;
        };
        self.play_from_file(path, self.mix_volume(0.34, SoundBus::Sfx), &family);
    }

    pub fn play_player_hurt(&mut self) {
        let Some((family, path)) = pick_sound_any(
            &self.misc_sounds,
            &mut self.misc_cursors,
            &self.break_sounds,
            &mut self.break_cursors,
            &self.walk_sounds,
            &mut self.walk_cursors,
            &["hit", "hurt", "damage", "legacy", "fall", "classic_hurt", "break"],
        ) else {
            return;
        };
        self.play_from_file(path, self.mix_volume(0.42, SoundBus::Sfx), &family);
    }

    pub fn play_ambient_family(&mut self, families: &'static [&'static str], volume: f32) {
        let Some((family, path)) = pick_sound_any(
            &self.misc_sounds,
            &mut self.misc_cursors,
            &self.break_sounds,
            &mut self.break_cursors,
            &self.walk_sounds,
            &mut self.walk_cursors,
            families,
        ) else {
            return;
        };
        self.play_from_file(
            path,
            self.mix_volume(volume.clamp(0.05, 0.65), SoundBus::Ambient),
            &family,
        );
    }

    pub fn play_cave_mood(&mut self, volume: f32) {
        let families = ["cave"];
        if let Some((family, path)) =
            pick_random_sound_from_map(&self.misc_sounds, &mut self.rng_state, &families)
        {
            self.play_from_file(
                path,
                self.mix_volume(volume.clamp(0.05, 0.65), SoundBus::Ambient),
                &family,
            );
            return;
        }

        self.play_ambient_family(&["cave", "stone"], volume);
    }

    pub fn play_background_music(&mut self, volume: f32) {
        let families = ["classic", "music", "bgm", "ambient"];
        if let Some((family, path)) =
            pick_random_sound_from_map(&self.misc_sounds, &mut self.rng_state, &families)
        {
            self.play_from_file(
                path,
                self.mix_volume(volume.clamp(0.04, 0.35), SoundBus::Music),
                &family,
            );
        }
    }

    pub fn play_item_pickup(&mut self) {
        let Some((family, path)) = pick_sound_any(
            &self.misc_sounds,
            &mut self.misc_cursors,
            &self.break_sounds,
            &mut self.break_cursors,
            &self.walk_sounds,
            &mut self.walk_cursors,
            &["pickup", "pop", "item"],
        ) else {
            return;
        };
        self.play_from_file(path, self.mix_volume(0.30, SoundBus::Sfx), &family);
    }

    fn play_from_file(&self, path: &Path, volume: f32, family: &str) {
        if volume <= 0.001 {
            return;
        }
        let file = match File::open(path) {
            Ok(f) => f,
            Err(err) => {
                log::warn!("Failed to open break sound {:?}: {}", path, err);
                return;
            }
        };

        let decoder = match Decoder::new(BufReader::new(file)) {
            Ok(d) => d,
            Err(err) => {
                log::warn!("Failed to decode break sound {:?}: {}", path, err);
                return;
            }
        };

        let sink = match Sink::try_new(&self.handle) {
            Ok(s) => s,
            Err(err) => {
                log::warn!("Failed to create audio sink for '{}': {}", family, err);
                return;
            }
        };

        sink.set_volume(volume);
        sink.append(decoder);
        sink.detach();
    }

    fn mix_volume(&self, base: f32, bus: SoundBus) -> f32 {
        let bus_gain = match bus {
            SoundBus::Music => self.music_volume,
            SoundBus::Ambient => self.ambient_volume,
            SoundBus::Sfx => self.sfx_volume,
        };
        (base * self.master_volume * bus_gain).clamp(0.0, 1.0)
    }
}

fn pick_sound_from_map<'a>(
    map: &'a HashMap<String, Vec<PathBuf>>,
    cursors: &mut HashMap<String, usize>,
    families: &[&str],
) -> Option<(String, &'a Path)> {
    for family in families {
        let Some(sounds) = map.get(*family) else {
            continue;
        };
        if sounds.is_empty() {
            continue;
        }

        let cursor = cursors.entry((*family).to_string()).or_insert(0);
        let idx = *cursor % sounds.len();
        *cursor = cursor.wrapping_add(1);
        return Some(((*family).to_string(), sounds[idx].as_path()));
    }
    None
}

fn pick_sound_any<'a>(
    misc: &'a HashMap<String, Vec<PathBuf>>,
    misc_cursors: &mut HashMap<String, usize>,
    break_map: &'a HashMap<String, Vec<PathBuf>>,
    break_cursors: &mut HashMap<String, usize>,
    walk_map: &'a HashMap<String, Vec<PathBuf>>,
    walk_cursors: &mut HashMap<String, usize>,
    families: &[&str],
) -> Option<(String, &'a Path)> {
    if let Some(v) = pick_sound_from_map(misc, misc_cursors, families) {
        return Some(v);
    }
    if let Some(v) = pick_sound_from_map(break_map, break_cursors, families) {
        return Some(v);
    }
    pick_sound_from_map(walk_map, walk_cursors, families)
}

fn load_sound_map(suffix: &str) -> HashMap<String, Vec<PathBuf>> {
    let mut map: HashMap<String, Vec<PathBuf>> = HashMap::new();
    let dir = sound_root();
    let mut files = Vec::new();
    collect_mp3_files_recursive(&dir, &mut files);
    if files.is_empty() {
        return map;
    }

    for path in files {
        let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else {
            continue;
        };
        if !stem.ends_with(suffix) {
            if let Some(family) = detect_block_family_from_path(&path, suffix) {
                map.entry(family).or_default().push(path);
            }
            continue;
        }

        let base = stem.trim_end_matches(suffix);
        let family = detect_block_family_from_path(&path, suffix)
            .unwrap_or_else(|| strip_trailing_digits(base).to_lowercase());
        map.entry(family).or_default().push(path);
    }

    map
}

fn load_misc_sound_map() -> HashMap<String, Vec<PathBuf>> {
    let mut map: HashMap<String, Vec<PathBuf>> = HashMap::new();
    let dir = sound_root();
    let mut files = Vec::new();
    collect_mp3_files_recursive(&dir, &mut files);
    if files.is_empty() {
        return map;
    }

    for path in files {
        let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else {
            continue;
        };
        if stem.ends_with("_break") || stem.ends_with("_walk") {
            continue;
        }

        let family = classify_misc_family(&path, stem);
        map.entry(family).or_default().push(path);
    }

    map
}

fn classify_misc_family(path: &Path, stem: &str) -> String {
    let stem_family = strip_trailing_digits(stem).to_lowercase();
    if stem_family.starts_with("thunder") {
        return "thunder".to_string();
    }
    if stem_family.starts_with("rain") {
        return "rain".to_string();
    }
    if stem_family.starts_with("cave") {
        return "cave".to_string();
    }
    if stem_family.starts_with("hit")
        || stem_family.starts_with("fall")
        || stem_family == "legacy"
        || stem_family == "hurt"
        || stem_family == "damage"
    {
        return "hit".to_string();
    }
    if stem_family == "click" {
        return "gui".to_string();
    }

    detect_misc_family_from_path(path).unwrap_or(stem_family)
}

fn sound_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("src")
        .join("assets")
        .join("sound")
}

fn collect_mp3_files_recursive(dir: &Path, out: &mut Vec<PathBuf>) {
    let entries = match fs::read_dir(dir) {
        Ok(v) => v,
        Err(err) => {
            log::warn!("Cannot read sound directory {:?}: {}", dir, err);
            return;
        }
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_mp3_files_recursive(&path, out);
            continue;
        }
        if !path.is_file() {
            continue;
        }
        let Some(ext) = path.extension().and_then(|s| s.to_str()) else {
            continue;
        };
        if ext.eq_ignore_ascii_case("mp3") {
            out.push(path);
        }
    }
}

fn detect_block_family_from_path(path: &Path, suffix: &str) -> Option<String> {
    let expected_kind = match suffix {
        "_break" => "break",
        "_walk" => "walk",
        _ => return None,
    };

    let parts: Vec<String> = path
        .components()
        .filter_map(|c| c.as_os_str().to_str())
        .map(|s| s.to_lowercase())
        .collect();

    for i in 0..parts.len() {
        if parts[i] != "block" {
            continue;
        }
        if i + 2 >= parts.len() {
            continue;
        }
        let family = &parts[i + 1];
        let kind = &parts[i + 2];
        if !family.is_empty() && kind == expected_kind {
            return Some(family.clone());
        }
    }
    None
}

fn detect_misc_family_from_path(path: &Path) -> Option<String> {
    let parts: Vec<String> = path
        .components()
        .filter_map(|c| c.as_os_str().to_str())
        .map(|s| s.to_lowercase())
        .collect();

    for i in 0..parts.len() {
        if (parts[i] == "ambient" || parts[i] == "misc") && i + 1 < parts.len() {
            let family = parts[i + 1].trim();
            if !family.is_empty() {
                return Some(family.to_string());
            }
        }
        if parts[i] == "block" && i + 1 < parts.len() {
            let family = parts[i + 1].trim();
            if !family.is_empty() {
                return Some(family.to_string());
            }
        }
    }
    None
}

fn pick_random_sound_from_map<'a>(
    map: &'a HashMap<String, Vec<PathBuf>>,
    rng_state: &mut u32,
    families: &[&str],
) -> Option<(String, &'a Path)> {
    for family in families {
        let Some(sounds) = map.get(*family) else {
            continue;
        };
        if sounds.is_empty() {
            continue;
        }
        let idx = random_index(rng_state, sounds.len());
        return Some(((*family).to_string(), sounds[idx].as_path()));
    }
    None
}

fn random_index(rng_state: &mut u32, len: usize) -> usize {
    if len <= 1 {
        return 0;
    }
    let mut x = *rng_state;
    if x == 0 {
        x = 0x9E37_79B9;
    }
    x ^= x << 13;
    x ^= x >> 17;
    x ^= x << 5;
    *rng_state = x;
    (x as usize) % len
}

fn break_sound_families(block: Block) -> &'static [&'static str] {
    match block {
        Block::Air | Block::CaveAir | Block::Water | Block::Stick | Block::Coal => &[],
        Block::Grass => &["grass", "gravel", "sand", "stone", "break"],
        Block::Dirt | Block::FarmlandDry | Block::FarmlandWet => &["gravel", "grass", "sand", "stone", "break"],
        Block::Sand => &["sand", "gravel", "grass", "stone", "break"],
        Block::Leaves => &["gravel", "grass", "sand", "stone", "break"],
        Block::Log | Block::LogBottom | Block::Workbench | Block::Wood => {
            &["wood", "grass", "gravel", "stone", "break"]
        }
        Block::Stone
        | Block::Furnace
        | Block::Bedrock
        | Block::CoalOre
        | Block::IronOre
        | Block::CopperOre => &["stone", "gravel", "sand", "grass", "break"],
    }
}

fn walk_sound_families(block: Block) -> &'static [&'static str] {
    match block {
        Block::Air | Block::CaveAir | Block::Water | Block::Stick | Block::Coal => &[],
        Block::Grass | Block::Leaves => &["grass", "gravel", "sand", "stone"],
        Block::Dirt | Block::FarmlandDry | Block::FarmlandWet => &["gravel", "grass", "sand", "stone"],
        Block::Sand => &["sand", "gravel", "stone", "grass"],
        Block::Log | Block::LogBottom | Block::Workbench | Block::Wood => {
            &["wood", "grass", "stone", "sand"]
        }
        Block::Stone
        | Block::Furnace
        | Block::Bedrock
        | Block::CoalOre
        | Block::IronOre
        | Block::CopperOre => &["stone", "gravel", "sand", "wood"],
    }
}

fn strip_trailing_digits(s: &str) -> &str {
    let mut end = s.len();
    for (idx, ch) in s.char_indices().rev() {
        if ch.is_ascii_digit() {
            end = idx;
            continue;
        }
        break;
    }
    if end == 0 || end == s.len() {
        s
    } else {
        &s[..end]
    }
}
