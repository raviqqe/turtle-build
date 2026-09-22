/// Run options.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RunOptions {
    /// Shows debug logs.
    pub debug: bool,
    /// Shows profile timings.
    pub profile: bool,
    /// A job limit.
    pub job_limit: usize,
}
