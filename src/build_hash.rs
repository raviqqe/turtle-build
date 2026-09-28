#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BuildHash {
    timestamp: u64,
    content: u64,
}

impl BuildHash {
    pub const fn new(timestamp: u64, content: u64) -> Self {
        Self { timestamp, content }
    }

    pub const fn timestamp(&self) -> u64 {
        self.timestamp
    }

    pub const fn content(&self) -> u64 {
        self.content
    }
}
