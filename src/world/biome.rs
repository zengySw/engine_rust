
use crate::world::block::Block;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[allow(dead_code)]
pub enum Biome {
    DeepFrozenOcean, FrozenOcean,
    DeepColdOcean, ColdOcean,
    Ocean, DeepOcean,
    LukewarmOcean, DeepLukewarmOcean,
    WarmOcean,
    Beach, StonyShore,
    River, FrozenRiver,
    SnowyPlains, IceSpikes, SnowyTaiga, SnowySlopes, FrozenPeaks,
    Taiga, OldGrowthPineTaiga, OldGrowthSpruceTaiga,
    Grove, JaggedPeaks, WindsweptGravellyHills,
    Plains, SunflowerPlains,
    Forest, FlowerForest,
    BirchForest, OldGrowthBirchForest,
    OldGrowthOakForest, DarkForest,
    Swamp, Meadow,
    WindsweptHills, WindsweptForest, StonyPeaks,
    Savanna, SavannaPlateau, WindsweptSavanna,
    Jungle, SparseJungle, BambooJungle, OldGrowthJungle,
    MangroveSwamp,
    Desert, Badlands, ErodedBadlands, WoodedBadlands,
    LushCaves, DripstoneCaves, DeepDark,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum TempCat {
    Frozen,     // < -0.45
    Cold,       // -0.45 .. -0.15
    Temperate,  // -0.15 ..  0.20
    Warm,       //  0.20 ..  0.55
    Hot,        // > 0.55
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum HumidCat {
    Arid,      // < -0.35
    Dry,       // -0.35 .. -0.10
    Moderate,  // -0.10 ..  0.10
    Wet,       //  0.10 ..  0.30
    Humid,     // > 0.30
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum ContCat {
    DeepOcean,
    Ocean,
    Coast,
    NearInland,
    MidInland,
    FarInland,
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
enum ErosionCat { E0, E1, E2, E3, E4, E5, E6 }

fn temp_cat(t: f64) -> TempCat {
    if      t < -0.45 { TempCat::Frozen }
    else if t < -0.15 { TempCat::Cold }
    else if t <  0.20 { TempCat::Temperate }
    else if t <  0.55 { TempCat::Warm }
    else              { TempCat::Hot }
}

fn humid_cat(h: f64) -> HumidCat {
    if      h < -0.35 { HumidCat::Arid }
    else if h < -0.10 { HumidCat::Dry }
    else if h <  0.10 { HumidCat::Moderate }
    else if h <  0.30 { HumidCat::Wet }
    else              { HumidCat::Humid }
}

fn cont_cat(c: f64) -> ContCat {
    if      c < -0.45 { ContCat::DeepOcean }
    else if c < -0.19 { ContCat::Ocean }
    else if c <  0.03 { ContCat::Coast }
    else if c <  0.30 { ContCat::NearInland }
    else if c <  0.55 { ContCat::MidInland }
    else              { ContCat::FarInland }
}

fn erosion_cat(e: f64) -> ErosionCat {
    if      e < -0.78   { ErosionCat::E0 }
    else if e < -0.375  { ErosionCat::E1 }
    else if e < -0.2225 { ErosionCat::E2 }
    else if e <  0.05   { ErosionCat::E3 }
    else if e <  0.45   { ErosionCat::E4 }
    else if e <  0.55   { ErosionCat::E5 }
    else                { ErosionCat::E6 }
}

// â”€â”€â”€ Ð“Ð»Ð°Ð²Ð½Ð°Ñ Ñ„ÑƒÐ½ÐºÑ†Ð¸Ñ Ð²Ñ‹Ð±Ð¾Ñ€Ð° Ð±Ð¸Ð¾Ð¼Ð° â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

pub fn select_biome(
    temperature:     f64,
    humidity:        f64,
    continentalness: f64,
    erosion:         f64,
    weirdness:       f64,
) -> Biome {
    use TempCat::*;
    use HumidCat::*;
    use ContCat::*;
    use ErosionCat::*;

    let t = temp_cat(temperature);
    let h = humid_cat(humidity);
    let c = cont_cat(continentalness);
    let e = erosion_cat(erosion);
    let w = weirdness;

    if c == DeepOcean || c == Ocean {
        let deep = c == DeepOcean;
        return match t {
            Frozen    => if deep { Biome::DeepFrozenOcean }   else { Biome::FrozenOcean },
            Cold      => if deep { Biome::DeepColdOcean }     else { Biome::ColdOcean },
            Temperate => if deep { Biome::DeepOcean }         else { Biome::Ocean },
            Warm      => if deep { Biome::DeepLukewarmOcean } else { Biome::LukewarmOcean },
            Hot       => Biome::WarmOcean,
        };
    }

    if c == Coast {
        return Biome::Beach;
    }

    if e == E6 && (c == NearInland || c == MidInland) {
        return match t {
            Frozen => Biome::FrozenRiver,
            _      => Biome::River,
        };
    }

    if c == FarInland && (e == E0 || e == E1) {
        return match t {
            Frozen    => Biome::FrozenPeaks,
            Cold      => Biome::JaggedPeaks,
            Temperate => if w > 0.0 { Biome::JaggedPeaks } else { Biome::StonyPeaks },
            Warm | Hot => Biome::StonyPeaks,
        };
    }

    if c == FarInland && e == E2 {
        return match (t, h) {
            (Frozen, _)            => Biome::SnowySlopes,
            (Cold, Humid | Wet)    => Biome::Grove,
            (Cold, _)              => Biome::SnowySlopes,
            (Temperate, _)         => Biome::Meadow,
            (Warm, _)              => Biome::Meadow,
            (Hot, _)               => Biome::Badlands,
        };
    }

   if c == MidInland && e == E0 {
        return match t {
            Frozen | Cold => Biome::FrozenPeaks,
            _             => Biome::StonyPeaks,
        };
    }

   if (e == E1 || e == E2) && w > 0.0 {
        return match t {
            Frozen | Cold => match h {
                Humid | Wet => Biome::WindsweptForest,
                _           => Biome::WindsweptHills,
            },
            Temperate => match h {
                Humid | Wet => Biome::WindsweptForest,
                _           => Biome::WindsweptHills,
            },
            Warm | Hot => Biome::WindsweptSavanna,
        };
    }

    if c == FarInland && e == E3 {
        return match (t, h) {
            (Frozen, _)             => Biome::SnowyPlains,
            (Cold, _)               => Biome::Taiga,
            (Temperate, Arid | Dry) => Biome::Meadow,
            (Temperate, _)          => Biome::Forest,
            (Warm, _)               => Biome::SavannaPlateau,
            (Hot, Arid)             => Biome::ErodedBadlands,
            (Hot, Dry)              => Biome::Badlands,
            (Hot, _)                => Biome::WoodedBadlands,
        };
    }

    if (e == E4 || e == E5) && c == NearInland && (h == Wet || h == Humid) {
        return match t {
            Warm | Hot => Biome::MangroveSwamp,
            _          => Biome::Swamp,
        };
    }

    inland_biome(t, h, w)
}

fn inland_biome(t: TempCat, h: HumidCat, w: f64) -> Biome {
    use TempCat::*;
    use HumidCat::*;
    match (t, h) {
        (Frozen, Arid | Dry)    => Biome::SnowyPlains,
        (Frozen, Moderate)      => if w > 0.30 { Biome::IceSpikes } else { Biome::SnowyPlains },
        (Frozen, Wet | Humid)   => Biome::SnowyTaiga,

        (Cold, Arid | Dry)      => Biome::Taiga,
        (Cold, Moderate)        => Biome::OldGrowthPineTaiga,
        (Cold, Wet | Humid)     => Biome::OldGrowthSpruceTaiga,

        (Temperate, Arid)       => Biome::Plains,
        (Temperate, Dry)        => if w > 0.40 { Biome::SunflowerPlains } else { Biome::Plains },
        (Temperate, Moderate)   => if w > 0.40 { Biome::FlowerForest }    else { Biome::Forest },
        (Temperate, Wet)        => if w > 0.30 { Biome::OldGrowthBirchForest } else { Biome::BirchForest },
        (Temperate, Humid)      => if w > 0.30 { Biome::OldGrowthOakForest }   else { Biome::DarkForest },

        (Warm, Arid | Dry)      => Biome::Savanna,
        (Warm, Moderate)        => Biome::SparseJungle,
        (Warm, Wet)             => Biome::Jungle,
        (Warm, Humid)           => if w > 0.30 { Biome::OldGrowthJungle } else { Biome::BambooJungle },

        (Hot, Arid)             => Biome::Desert,
        (Hot, Dry)              => if w > 0.40 { Biome::ErodedBadlands } else { Biome::Badlands },
        (Hot, Moderate)         => Biome::WoodedBadlands,
        (Hot, Wet | Humid)      => Biome::Jungle,
    }
}

// â”€â”€â”€ ÐŸÐ¾Ð´Ð·ÐµÐ¼Ð½Ñ‹Ðµ Ð±Ð¸Ð¾Ð¼Ñ‹ â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

#[allow(dead_code)]
pub fn select_cave_biome(temperature: f64, humidity: f64, depth: i32) -> Biome {
    use TempCat::*;
    use HumidCat::*;
    if depth > 150 { return Biome::DeepDark; }
    match (temp_cat(temperature), humid_cat(humidity)) {
        (Temperate | Warm, Wet | Humid) => Biome::LushCaves,
        (Hot | Warm, Arid | Dry)        => Biome::DripstoneCaves,
        _                               => Biome::DripstoneCaves,
    }
}

// â”€â”€â”€ Ð¡Ð²Ð¾Ð¹ÑÑ‚Ð²Ð° Ð±Ð¸Ð¾Ð¼Ð¾Ð² â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

#[derive(Clone, Copy, Debug)]
pub struct SurfaceProfile {
    pub top: Block,
    pub filler: Block,
    pub depth: usize,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum SurfaceClass {
    Grassy,
    Sandy,
    Rocky,
}

fn surface_profile(biome: Biome) -> SurfaceProfile {
    match biome {
        Biome::Desert | Biome::Beach | Biome::StonyShore
        | Biome::Badlands | Biome::ErodedBadlands
        | Biome::WoodedBadlands
        | Biome::WarmOcean | Biome::River | Biome::FrozenRiver
        | Biome::Ocean | Biome::DeepOcean
        | Biome::ColdOcean | Biome::DeepColdOcean
        | Biome::LukewarmOcean | Biome::DeepLukewarmOcean
        | Biome::FrozenOcean | Biome::DeepFrozenOcean => SurfaceProfile {
            top: Block::Sand,
            filler: Block::Sand,
            depth: 4,
        },

        Biome::JaggedPeaks | Biome::StonyPeaks | Biome::FrozenPeaks
        | Biome::LushCaves | Biome::DripstoneCaves | Biome::DeepDark => SurfaceProfile {
            top: Block::Stone,
            filler: Block::Stone,
            depth: 5,
        },

        _ => SurfaceProfile {
            top: Block::Grass,
            filler: Block::Dirt,
            depth: 4,
        },
    }
}

fn surface_class(biome: Biome) -> SurfaceClass {
    let p = surface_profile(biome);
    if p.top == Block::Sand {
        SurfaceClass::Sandy
    } else if p.top == Block::Stone {
        SurfaceClass::Rocky
    } else {
        SurfaceClass::Grassy
    }
}

#[inline]
fn hash01(h: u32) -> f32 {
    (h & 0xffff) as f32 / 65535.0
}

fn dither_profile(a: SurfaceProfile, b: SurfaceProfile, blend: f32, hash: u32) -> SurfaceProfile {
    let mut out = a;
    let t = blend.clamp(0.0, 1.0);

    if hash01(hash) < t {
        out.top = b.top;
    }
    if hash01(hash.rotate_left(7)) < t * 0.90 {
        out.filler = b.filler;
    }

    let depth = (a.depth as f32 * (1.0 - t * 0.75) + b.depth as f32 * (t * 0.75)).round() as usize;
    out.depth = depth.clamp(2, 7);
    out
}

#[inline]
fn smoothstep01(x: f32) -> f32 {
    let t = x.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

pub fn blended_surface_profile_weighted(
    center: Biome,
    neighbors: [Biome; 8],        // [W, E, N, S, NW, NE, SW, SE]
    center_height: u32,
    neighbor_heights: [u32; 8],   // same order as neighbors
    cardinal_slope: i32,
    sea_level: u32,
    hash: u32,
) -> SurfaceProfile {
    let center_profile = surface_profile(center);

    let mut different = 0usize;
    for b in neighbors {
        if b != center {
            different += 1;
        }
    }
    if different == 0 && cardinal_slope <= 1 {
        return center_profile;
    }

    const DIST_W: [f32; 8] = [1.0, 1.0, 1.0, 1.0, 0.72, 0.72, 0.72, 0.72];
    let mut dominant = center;
    let mut dominant_score = 0.0f32;

    for i in 0..neighbors.len() {
        let candidate = neighbors[i];
        if candidate == center {
            continue;
        }

        let mut score = 0.0f32;
        for j in 0..neighbors.len() {
            if neighbors[j] != candidate {
                continue;
            }
            let dh = (neighbor_heights[j] as i32 - center_height as i32).abs() as f32;
            let h_w = (1.0 - dh / 20.0).clamp(0.15, 1.0);
            score += DIST_W[j] * h_w;
        }

        if score > dominant_score || (score == dominant_score && ((hash >> (i * 3)) & 1) == 1) {
            dominant = candidate;
            dominant_score = score;
        }
    }

    let mut out = center_profile;
    if dominant != center {
        let diversity = different as f32 / 8.0;
        let score_norm = (dominant_score / 6.88).clamp(0.0, 1.0);
        let mut blend = 0.58 * diversity + 0.42 * score_norm;

        if surface_class(center) != surface_class(dominant) {
            blend = (blend * 1.18).min(1.0);
        }

        if is_ocean(center) ^ is_ocean(dominant) {
            let beach = SurfaceProfile { top: Block::Sand, filler: Block::Sand, depth: 3 };
            let shore_bias = if center_height <= sea_level + 3 { 0.82 } else { 0.56 };
            out = dither_profile(center_profile, beach, blend.max(shore_bias), hash ^ 0x9E37_79B9);
        } else {
            out = dither_profile(center_profile, surface_profile(dominant), blend, hash);
        }
    }

    let mountain_neighbors = neighbors.iter().filter(|&&b| is_mountain(b)).count();
    let mountainish = is_mountain(center) || mountain_neighbors >= 3;
    let steep_thr = if mountainish { 5 } else { 4 };
    let high_thr = if mountainish { sea_level + 68 } else { sea_level + 92 };

    if cardinal_slope >= steep_thr || center_height >= high_thr {
        let slope_blend = smoothstep01((cardinal_slope as f32 - (steep_thr as f32 - 0.4)) / 4.4);
        let height_blend = smoothstep01((center_height.saturating_sub(high_thr) as f32) / 56.0);
        let rock_blend = slope_blend.max(height_blend) * 0.60;
        let rock = SurfaceProfile { top: Block::Stone, filler: Block::Stone, depth: 5 };
        out = dither_profile(out, rock, rock_blend, hash.rotate_left(11));
    }

    out
}

#[allow(dead_code)]
pub fn blended_surface_profile(center: Biome, neighbors: [Biome; 4], hash: u32) -> SurfaceProfile {
    let center_profile = surface_profile(center);

    let mut different = 0usize;
    for b in neighbors {
        if b != center {
            different += 1;
        }
    }
    if different == 0 {
        return center_profile;
    }

    let mut dominant = center;
    let mut dominant_count = 0usize;
    for i in 0..neighbors.len() {
        let b = neighbors[i];
        if b == center {
            continue;
        }
        let mut count = 1usize;
        for &other in neighbors.iter().skip(i + 1) {
            if other == b {
                count += 1;
            }
        }
        if count > dominant_count || (count == dominant_count && ((hash >> (i * 3)) & 1) == 1) {
            dominant = b;
            dominant_count = count;
        }
    }
    if dominant_count == 0 {
        return center_profile;
    }

    let mut blend = different as f32 / 4.0;
    if surface_class(center) != surface_class(dominant) {
        blend = (blend * 1.15).min(1.0);
    }

    if is_ocean(center) ^ is_ocean(dominant) {
        let beach = SurfaceProfile { top: Block::Sand, filler: Block::Sand, depth: 3 };
        return dither_profile(center_profile, beach, blend.max(0.55), hash ^ 0x9E37_79B9);
    }

    let dominant_profile = surface_profile(dominant);
    dither_profile(center_profile, dominant_profile, blend, hash)
}

#[allow(dead_code)]
pub fn top_block(biome: Biome) -> Block {
    surface_profile(biome).top
}

#[allow(dead_code)]
pub fn filler_block(biome: Biome) -> Block {
    surface_profile(biome).filler
}

#[allow(dead_code)]
pub fn filler_depth(biome: Biome) -> usize {
    surface_profile(biome).depth
}
pub fn tree_chance(biome: Biome) -> f32 {
    match biome {
        Biome::OldGrowthOakForest | Biome::OldGrowthBirchForest
        | Biome::OldGrowthPineTaiga | Biome::OldGrowthSpruceTaiga
        | Biome::OldGrowthJungle | Biome::DarkForest => 0.12,

        Biome::BambooJungle | Biome::Jungle
        | Biome::BirchForest | Biome::Forest
        | Biome::FlowerForest => 0.07,

        Biome::SnowyTaiga | Biome::Taiga | Biome::Grove
        | Biome::WoodedBadlands | Biome::SparseJungle
        | Biome::WindsweptForest => 0.04,

        Biome::Plains | Biome::SunflowerPlains
        | Biome::Swamp | Biome::MangroveSwamp
        | Biome::Savanna | Biome::SavannaPlateau => 0.02,

        Biome::SnowyPlains | Biome::IceSpikes | Biome::Meadow => 0.005,

        _ => 0.0,
    }
}

pub fn tree_height_range(biome: Biome) -> (usize, usize) {
    match biome {
        Biome::Jungle | Biome::OldGrowthJungle | Biome::BambooJungle => (8, 14),
        Biome::OldGrowthOakForest | Biome::OldGrowthBirchForest
        | Biome::OldGrowthPineTaiga | Biome::OldGrowthSpruceTaiga
        | Biome::DarkForest => (7, 11),
        Biome::Taiga | Biome::SnowyTaiga | Biome::Grove => (6, 9),
        _ => (4, 6),
    }
}

#[derive(Clone, Copy, Debug)]
pub struct VegetationProfile {
    pub tree_chance: f32,
    pub min_height: usize,
    pub max_height: usize,
    pub crown_radius: i32,
}

fn vegetation_profile(biome: Biome) -> VegetationProfile {
    let (min_height, max_height) = tree_height_range(biome);
    let crown_radius = match biome {
        Biome::Jungle | Biome::OldGrowthJungle | Biome::BambooJungle => 3,
        Biome::DarkForest | Biome::OldGrowthOakForest | Biome::OldGrowthBirchForest => 3,
        Biome::OldGrowthPineTaiga | Biome::OldGrowthSpruceTaiga => 2,
        _ => 2,
    };
    VegetationProfile {
        tree_chance: tree_chance(biome),
        min_height,
        max_height,
        crown_radius,
    }
}

#[inline]
fn lerp_f32(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

#[inline]
fn lerp_usize(a: usize, b: usize, t: f32) -> usize {
    lerp_f32(a as f32, b as f32, t).round() as usize
}

pub fn blended_vegetation_profile_weighted(
    center: Biome,
    neighbors: [Biome; 8],      // [W, E, N, S, NW, NE, SW, SE]
    center_height: u32,
    neighbor_heights: [u32; 8], // same order as neighbors
    cardinal_slope: i32,
    sea_level: u32,
    hash: u32,
) -> VegetationProfile {
    let center_v = vegetation_profile(center);
    if center_v.tree_chance <= 0.0 && neighbors.iter().all(|&b| tree_chance(b) <= 0.0) {
        return center_v;
    }

    const DIST_W: [f32; 8] = [1.0, 1.0, 1.0, 1.0, 0.72, 0.72, 0.72, 0.72];
    let mut dominant = center;
    let mut dominant_score = 0.0f32;

    for i in 0..neighbors.len() {
        let candidate = neighbors[i];
        if candidate == center {
            continue;
        }

        let mut score = 0.0f32;
        for j in 0..neighbors.len() {
            if neighbors[j] != candidate {
                continue;
            }
            let dh = (neighbor_heights[j] as i32 - center_height as i32).abs() as f32;
            let h_w = (1.0 - dh / 18.0).clamp(0.12, 1.0);
            score += DIST_W[j] * h_w;
        }

        if score > dominant_score || (score == dominant_score && ((hash >> (i * 3)) & 1) == 1) {
            dominant = candidate;
            dominant_score = score;
        }
    }

    let dominant_v = vegetation_profile(dominant);
    let mut different = 0usize;
    for b in neighbors {
        if b != center {
            different += 1;
        }
    }
    let diversity = different as f32 / 8.0;
    let score_norm = (dominant_score / 6.88).clamp(0.0, 1.0);
    let blend = (0.62 * diversity + 0.38 * score_norm).clamp(0.0, 1.0);

    let mut out = center_v;
    out.tree_chance = lerp_f32(center_v.tree_chance, dominant_v.tree_chance, blend);
    out.min_height = lerp_usize(center_v.min_height, dominant_v.min_height, blend * 0.85);
    out.max_height = lerp_usize(center_v.max_height, dominant_v.max_height, blend * 0.85)
        .max(out.min_height + 1);
    out.crown_radius = if blend > 0.6 {
        dominant_v.crown_radius.max(center_v.crown_radius)
    } else {
        center_v.crown_radius
    };

    if is_ocean(center) || center_height <= sea_level + 2 {
        out.tree_chance *= 0.20;
    }

    // Smooth suppression on rough terrain instead of hard cutoff.
    let slope_factor = 1.0 - smoothstep01((cardinal_slope as f32 - 1.5) / 4.5) * 0.82;
    out.tree_chance *= slope_factor.clamp(0.12, 1.0);

    if is_mountain(center) {
        let alpine = smoothstep01((center_height.saturating_sub(sea_level + 44) as f32) / 48.0);
        out.tree_chance *= lerp_f32(0.86, 0.28, alpine);
    }

    // Tiny deterministic jitter to avoid linear transition bands.
    let jitter = 0.90 + 0.20 * hash01(hash.rotate_left(9));
    out.tree_chance = (out.tree_chance * jitter).clamp(0.0, 0.18);

    out
}

pub fn is_ocean(biome: Biome) -> bool {
    matches!(biome,
        Biome::Ocean | Biome::DeepOcean |
        Biome::ColdOcean | Biome::DeepColdOcean |
        Biome::FrozenOcean | Biome::DeepFrozenOcean |
        Biome::LukewarmOcean | Biome::DeepLukewarmOcean |
        Biome::WarmOcean
    )
}

pub fn is_mountain(biome: Biome) -> bool {
    matches!(biome,
        Biome::JaggedPeaks | Biome::StonyPeaks | Biome::FrozenPeaks |
        Biome::SnowySlopes | Biome::Grove | Biome::Meadow |
        Biome::WindsweptHills | Biome::WindsweptForest |
        Biome::WindsweptGravellyHills | Biome::WindsweptSavanna |
        Biome::SavannaPlateau
    )
}

pub fn cave_threshold(biome: Biome) -> f64 {
    match biome {
        Biome::DripstoneCaves => 0.55,
        Biome::LushCaves      => 0.58,
        Biome::DeepDark       => 0.52,
        _ if is_mountain(biome) => 0.57,
        _ => 0.62,
    }
}
