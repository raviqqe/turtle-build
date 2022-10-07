#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Options {
    pub debug: bool,
    pub job_limit: Option<usize>,
    pub profile: bool,
}
