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
}

impl Block {
    pub fn is_solid(&self) -> bool {
        !matches!(self, Block::Air | Block::Water)
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
        }
    }
}