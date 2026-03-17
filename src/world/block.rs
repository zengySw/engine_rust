#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[allow(dead_code)]
pub enum Block {
    Air,
    CaveAir,
    Workbench,
    Furnace,
    Coal,
    IronIngot,
    Torch,
    Wood,
    Stick,
    Grass,
    Dirt,
    FarmlandDry,
    FarmlandWet,
    Stone,
    Sand,
    Water,
    Bedrock,
    Log,
    Leaves,
    LogBottom,
    CoalOre,
    IronOre,
    CopperOre,
}

impl Block {
    pub fn is_air(&self) -> bool {
        matches!(self, Block::Air | Block::CaveAir)
    }

    pub fn is_placeable(&self) -> bool {
        !matches!(
            self,
            Block::Air
                | Block::CaveAir
                | Block::Water
                | Block::Stick
                | Block::Coal
                | Block::IronIngot
        )
    }

    pub fn is_solid(&self) -> bool {
        !matches!(
            self,
            Block::Air
                | Block::CaveAir
                | Block::Water
                | Block::Stick
                | Block::Coal
                | Block::IronIngot
                | Block::Torch
        )
    }

    pub fn is_breakable(&self) -> bool {
        !matches!(
            self,
            Block::Air
                | Block::CaveAir
                | Block::Water
                | Block::Bedrock
                | Block::Stick
                | Block::Coal
                | Block::IronIngot
        )
    }

    pub fn drop_item(&self) -> Option<Block> {
        match self {
            Block::Air | Block::CaveAir | Block::Water | Block::Bedrock => None,
            Block::Grass => Some(Block::Dirt),
            Block::FarmlandDry | Block::FarmlandWet => Some(Block::Dirt),
            Block::LogBottom => Some(Block::Log),
            Block::CoalOre => Some(Block::Coal),
            b => Some(*b),
        }
    }

    #[allow(dead_code)]
    pub fn texture_name(&self) -> &'static str {
        match self {
            Block::Air     => "air",
            Block::CaveAir => "air",
            Block::Workbench => "workbench",
            Block::Furnace => "furnace",
            Block::Coal => "coal",
            Block::IronIngot => "iron_ingot",
            Block::Torch => "torch",
            Block::Wood => "wood",
            Block::Stick => "stick",
            Block::Grass   => "grass",
            Block::Dirt    => "dirt",
            Block::FarmlandDry => "farmland_dry",
            Block::FarmlandWet => "farmland_wet",
            Block::Stone   => "stone",
            Block::Sand    => "sand",
            Block::Water   => "water",
            Block::Bedrock => "bedrock",
            Block::Log     => "log",
            Block::LogBottom => "logBottom",
            Block::Leaves  => "leaves",
            Block::CoalOre => "coal_ore",
            Block::IronOre => "iron_ore",
            Block::CopperOre => "copper_ore",
        }
    }

    pub fn item_key(&self) -> &'static str {
        match self {
            Block::Air => "air",
            Block::CaveAir => "cave_air",
            Block::Workbench => "workbench",
            Block::Furnace => "furnace",
            Block::Coal => "coal",
            Block::IronIngot => "iron_ingot",
            Block::Torch => "torch",
            Block::Wood => "wood",
            Block::Stick => "stick",
            Block::Grass => "grass",
            Block::Dirt => "dirt",
            Block::FarmlandDry => "farmland_dry",
            Block::FarmlandWet => "farmland_wet",
            Block::Stone => "stone",
            Block::Sand => "sand",
            Block::Water => "water",
            Block::Bedrock => "bedrock",
            Block::Log => "log",
            Block::Leaves => "leaves",
            Block::LogBottom => "log_bottom",
            Block::CoalOre => "coal_ore",
            Block::IronOre => "iron_ore",
            Block::CopperOre => "copper_ore",
        }
    }

    pub fn texture_index(&self) -> u32 {
        match self {
            Block::Air     => 0,
            Block::CaveAir => 0,
            Block::Workbench => 16,
            Block::Furnace => 21,
            Block::Coal => 24,
            Block::IronIngot => 26,
            Block::Torch => 25,
            Block::Wood => 17,
            Block::Stick => 18,
            Block::Grass   => 1,
            Block::Dirt    => 2,
            Block::Stone   => 3,
            Block::Sand    => 4,
            Block::Water   => 5,
            Block::Bedrock => 6,
            Block::Log     => 7,
            Block::LogBottom => 8,
            Block::Leaves  => 9,
            Block::CoalOre => 10,
            Block::IronOre => 11,
            Block::CopperOre => 12,
            Block::FarmlandDry => 13,
            Block::FarmlandWet => 14,
        }
    }

    pub fn id(&self) -> u8 {
        match self {
            Block::Air => 0,
            Block::CaveAir => 15,
            Block::Workbench => 16,
            Block::Furnace => 19,
            Block::Coal => 20,
            Block::IronIngot => 22,
            Block::Torch => 21,
            Block::Wood => 17,
            Block::Stick => 18,
            Block::Grass => 1,
            Block::Dirt => 2,
            Block::Stone => 3,
            Block::Sand => 4,
            Block::Water => 5,
            Block::Bedrock => 6,
            Block::Log => 7,
            Block::Leaves => 8,
            Block::LogBottom => 9,
            Block::CoalOre => 10,
            Block::IronOre => 11,
            Block::CopperOre => 12,
            Block::FarmlandDry => 13,
            Block::FarmlandWet => 14,
        }
    }

    pub fn from_id(id: u8) -> Option<Self> {
        match id {
            0 => Some(Block::Air),
            15 => Some(Block::CaveAir),
            16 => Some(Block::Workbench),
            19 => Some(Block::Furnace),
            20 => Some(Block::Coal),
            22 => Some(Block::IronIngot),
            21 => Some(Block::Torch),
            17 => Some(Block::Wood),
            18 => Some(Block::Stick),
            1 => Some(Block::Grass),
            2 => Some(Block::Dirt),
            3 => Some(Block::Stone),
            4 => Some(Block::Sand),
            5 => Some(Block::Water),
            6 => Some(Block::Bedrock),
            7 => Some(Block::Log),
            8 => Some(Block::Leaves),
            9 => Some(Block::LogBottom),
            10 => Some(Block::CoalOre),
            11 => Some(Block::IronOre),
            12 => Some(Block::CopperOre),
            13 => Some(Block::FarmlandDry),
            14 => Some(Block::FarmlandWet),
            _ => None,
        }
    }
}
