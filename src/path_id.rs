#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq)]
pub struct PathId(usize);

impl PathId {
    pub fn new(id: usize) -> Self {
        Self(id)
    }
}
