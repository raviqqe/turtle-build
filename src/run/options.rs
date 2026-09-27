/// Run options.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RunOptions {
    /// Shows debug logs.
    pub debug: bool,
    /// Runs builds without running commands.
    pub dry_run: bool,
    /// Shows profile timings.
    pub profile: bool,
}
