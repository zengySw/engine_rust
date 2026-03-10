#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Block {
    Air,
    Grass,
    Dirt,
    Stone,
    Sand,
    Water,
}

impl Block {
    pub fn is_solid(&self) -> bool {
        !matches!(self, Block::Air | Block::Water)
    }

    /// UV-индекс тайла в атласе текстур (колонка, строка)
    /// Потом заменим на реальный атлас
    pub fn texture_index(&self) -> u32 {
        match self {
            Block::Air   => 0,
            Block::Grass => 1,
            Block::Dirt  => 2,
            Block::Stone => 3,
            Block::Sand  => 4,
            Block::Water => 5,
        }
    }
}