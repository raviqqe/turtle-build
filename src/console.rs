use tokio::io::{stderr, stdout, Stderr, Stdout};

#[derive(Debug)]
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

    pub fn stdout(&mut self) -> &mut Stdout {
        &mut self.stdout
    }

    pub fn stderr(&mut self) -> &mut Stderr {
        &mut self.stderr
    }
}
