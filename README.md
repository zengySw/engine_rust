# Engine Rust

Experimental game engine written in Rust.

The goal of this project is to explore building a game engine from scratch while focusing on performance, security, and clean architecture.

---

## ⚠️ Important Notice

> **GPT tried to help with this project.**  
> It failed. Spectacularly.  
> Even with multi-step reasoning chains, chain-of-thought prompting, and paid tiers —  
> it still produced broken Rust code, hallucinated API signatures, and confidently  
> suggested solutions that don't compile.  
>
> Claude was used instead. The engine now works.  
> 🪦 R.I.P. GPT's Rust skills. Gone but not missed.

---

## Project Idea

This engine is being developed as a foundation for a Minecraft-like sandbox game.
The long-term vision is to create a flexible engine that can support:

- Procedural worlds with realistic biome transitions
- Block-based environments
- Mod support
- Customizable gameplay systems

The project is still in an early stage of development and many core systems are being designed and tested.

---

## Goals

- Build a lightweight and optimized engine
- Learn and experiment with Rust for game development
- Design a modular architecture
- Allow future support for mods and extensions
- Improve security and stability compared to typical hobby engines

---

## Development Status

⚠️ Early development — subject to major changes.

---

## Controls

- `W A S D` — move
- `Space` — jump
- `E` — inventory
- `Esc` — pause menu
- `F3` — debug overlay (Minecraft-style)

---

## World Generation

The world uses a multi-parameter climate system for realistic biome distribution, inspired by Minecraft Java Edition.

### Climate Parameters

Each point in the world is described by 5 independent noise fields:

| Parameter | Range | Controls |
|---|---|---|
| `temperature` | -1..1 | Frozen → Hot |
| `humidity` | -1..1 | Arid → Humid |
| `continentalness` | -1..1 | Deep ocean → Far inland |
| `erosion` | -1..1 | Peaks → Flatlands |
| `weirdness` | -1..1 | Normal → Rare variants |

### Biome Selection Algorithm

1. All 5 noise fields are sampled at the current world coordinate
2. Each value is categorized (e.g. temperature → Frozen/Cold/Neutral/Warm/Hot)
3. A decision tree selects the biome based on the combination of categories
4. Special rules apply for peaks, rivers, ocean depths, windswept terrain, etc.

### Terrain Blending

- Heightmaps are generated per-chunk and blended at chunk borders (2-block transition zone)
- Temperature decreases with altitude (lapse rate: -0.0026 per block above sea level)
- Cave generation threshold varies per biome

---

## Implemented Biomes

### 🌊 Ocean
`DeepFrozenOcean` · `FrozenOcean` · `DeepColdOcean` · `ColdOcean`  
`Ocean` · `DeepOcean` · `LukewarmOcean` · `DeepLukewarmOcean` · `WarmOcean`

### 🏖️ Beach & Shore
`Beach` · `StonyShore` · `River` · `FrozenRiver`

### 🧊 Frozen
`SnowyPlains` · `IceSpikes` · `SnowyTaiga` · `SnowySlopes` · `FrozenPeaks`

### 🌲 Cold
`Taiga` · `OldGrowthPineTaiga` · `OldGrowthSpruceTaiga` · `Grove` · `JaggedPeaks` · `WindsweptGravellyHills`

### 🌳 Temperate
`Plains` · `SunflowerPlains` · `Forest` · `FlowerForest`  
`BirchForest` · `OldGrowthBirchForest` · `OldGrowthOakForest` · `DarkForest`  
`Swamp` · `Meadow` · `WindsweptHills` · `WindsweptForest` · `StonyPeaks`

### 🌴 Warm
`Savanna` · `SavannaPlateau` · `WindsweptSavanna`  
`Jungle` · `SparseJungle` · `BambooJungle` · `OldGrowthJungle` · `MangroveSwamp`

### 🏜️ Hot & Dry
`Desert` · `Badlands` · `ErodedBadlands` · `WoodedBadlands`

### 🕳️ Underground
`LushCaves` · `DripstoneCaves` · `DeepDark`

---

## Architecture

```
src/
├── main.rs          — entry point
├── engine.rs        — game loop, input
├── renderer.rs      — wgpu rendering pipeline
├── camera.rs        — view/projection matrices
├── culling.rs       — frustum culling (rayon parallel)
├── player.rs        — physics, collision
├── inventory.rs     — hotbar + inventory UI (egui)
├── menu.rs          — pause menu (egui)
├── args.rs          — CLI arguments
└── world/
    ├── biome.rs     — climate system, all biome definitions
    ├── block.rs     — block types
    ├── chunk.rs     — terrain generation, meshing
    └── world.rs     — chunk streaming, thread pools
```

### Threading Model

| Pool | Threads | Task |
|---|---|---|
| `hmap` | threads/4 | Heightmap + biome generation |
| `gen`  | threads/4 | Block placement per chunk |
| `mesh` | threads/2 | Greedy mesh building |
| `cull` | threads/4 | Frustum culling |

---

## Getting Started

```bash
# Default run
cargo run --release

# Custom options
cargo run --release -- --threads 8 --render_dist 12 --vsync
```

---

## Contributing

This project is open to ideas and contributions. Feel free to open an issue or submit a pull request.

Ways to help:
- Improve world generation (structures, more biome detail)
- Add new block types (snow, terracotta, gravel, mud...)
- Implement biome-specific surface decoration
- Optimize meshing or chunk streaming
- Suggest better design approaches

---

## Vision

A clean, optimized, and secure sandbox engine that could eventually power a Minecraft-style game with modding capabilities.

---

