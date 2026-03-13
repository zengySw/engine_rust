#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[allow(dead_code)]
pub enum Block {
    Air,
    Grass,
    Dirt,
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
    pub fn is_solid(&self) -> bool {
        !matches!(self, Block::Air | Block::Water)
    }

    #[allow(dead_code)]
    pub fn texture_name(&self) -> &'static str {
        match self {
            Block::Air     => "air",
            Block::Grass   => "grass",
            Block::Dirt    => "dirt",
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

    pub fn texture_index(&self) -> u32 {
        match self {
            Block::Air     => 0,
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
        }
    }

    pub fn id(&self) -> u8 {
        match self {
            Block::Air => 0,
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
        }
    }

    pub fn from_id(id: u8) -> Option<Self> {
        match id {
            0 => Some(Block::Air),
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
            _ => None,
        }
    }
}
