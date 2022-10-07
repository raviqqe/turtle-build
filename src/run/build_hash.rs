pub struct BuildHash {
    timestamp: u64,
    content: u64,
}

impl BuildHash {
    pub fn new(timestamp: u64, content: u64) -> Self {
        Self { timestamp, content }
    }

    pub fn timestamp(&self) -> u64 {
        self.timestamp
    }

    pub fn content(&self) -> u64 {
        self.content
    }
}
