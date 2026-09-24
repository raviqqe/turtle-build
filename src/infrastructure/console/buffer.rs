use crate::infrastructure::{Console, ConsoleError};

#[derive(Debug)]
enum Chunk {
    Stdout(Vec<u8>),
    Stderr(Vec<u8>),
}

/// A buffer of outputs to a console.
#[derive(Debug, Default)]
pub struct ConsoleBuffer {
    chunks: Vec<Chunk>,
}

impl ConsoleBuffer {
    pub fn write_stdout(&mut self, bytes: &[u8]) {
        self.chunks.push(Chunk::Stdout(bytes.into()));
    }

    pub fn write_stderr(&mut self, bytes: &[u8]) {
        self.chunks.push(Chunk::Stderr(bytes.into()));
    }

    pub fn append(&mut self, other: Self) {
        self.chunks.extend(other.chunks);
    }

    pub async fn write_to(
        &self,
        console: &mut (dyn Console + Send + Sync),
    ) -> Result<(), ConsoleError> {
        for chunk in &self.chunks {
            match chunk {
                Chunk::Stdout(bytes) => console.write_stdout(bytes).await?,
                Chunk::Stderr(bytes) => console.write_stderr(bytes).await?,
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::infrastructure::FakeConsole;
    use pretty_assertions::assert_eq;

    async fn write(buffer: &ConsoleBuffer) -> FakeConsole {
        let mut console = FakeConsole::default();

        buffer.write_to(&mut console).await.unwrap();

        console
    }

    #[tokio::test]
    async fn write_nothing() {
        let console = write(&Default::default()).await;

        assert_eq!(console.stdout(), "");
        assert_eq!(console.stderr(), "");
    }

    #[tokio::test]
    async fn write_stdout() {
        let mut buffer = ConsoleBuffer::default();

        buffer.write_stdout(b"foo");

        assert_eq!(write(&buffer).await.stdout(), "foo");
    }

    #[tokio::test]
    async fn write_stderr() {
        let mut buffer = ConsoleBuffer::default();

        buffer.write_stderr(b"foo");

        assert_eq!(write(&buffer).await.stderr(), "foo");
    }

    #[tokio::test]
    async fn write_stdout_and_stderr() {
        let mut buffer = ConsoleBuffer::default();

        buffer.write_stdout(b"foo");
        buffer.write_stderr(b"bar");
        buffer.write_stdout(b"baz");

        let console = write(&buffer).await;

        assert_eq!(console.stdout(), "foobaz");
        assert_eq!(console.stderr(), "bar");
    }

    #[tokio::test]
    async fn append_buffer() {
        let mut buffer = ConsoleBuffer::default();
        let mut other = ConsoleBuffer::default();

        buffer.write_stdout(b"foo");
        other.write_stdout(b"bar");
        other.write_stderr(b"baz");
        buffer.append(other);

        let console = write(&buffer).await;

        assert_eq!(console.stdout(), "foobar");
        assert_eq!(console.stderr(), "baz");
    }
}
