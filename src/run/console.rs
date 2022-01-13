use tokio::io::{Stderr, Stdout};

pub struct Console {
    stdout: Stdout,
    stderr: Stderr,
}

impl Console {
    pub fn new() -> Self {
        Self {
            stdout: stdout(),
            stderr: stderr(),
        }
    }

    pub fn stdout() -> &mut Stdout {
        &mut self.stdout
    }

    pub fn stderr() -> &mut Stderr {
        &mut self.stderr
    }
}
