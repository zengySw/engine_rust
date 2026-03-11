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
        }
    }
}
