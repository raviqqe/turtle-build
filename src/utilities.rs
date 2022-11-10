use crate::error::ApplicationError;
use std::path::Path;
use tokio::{
    fs::File,
    io::{self, AsyncReadExt, AsyncWriteExt},
};

pub async fn read_file(path: impl AsRef<Path>) -> Result<String, ApplicationError<'static>> {
    let mut source = "".into();
    let path = path.as_ref();

    File::open(path)
        .await
        .map_err(|error| ApplicationError::with_path(error, path))?
        .read_to_string(&mut source)
        .await
        .map_err(|error| ApplicationError::with_path(error, path))?;

    Ok(source)
}

#[macro_export]
macro_rules! writeln {
    ($writer:expr, $template:literal, $($value:expr),+) => {
        $crate::utilities::writeln($writer, format!($template, $($value),+)).await?
    };
}

#[macro_export]
macro_rules! debug {
    ($debug:expr, $writer:expr, $template:literal, $($value:expr),+) => {
        if $debug {
            $crate::utilities::writeln(
                $writer,
                "turtle: ".to_owned() + &format!($template, $($value),+),
            ).await?;
        }
    };
}

pub async fn writeln(
    writer: &mut (impl AsyncWriteExt + Unpin),
    message: impl AsRef<str>,
) -> Result<(), io::Error> {
    writer.write_all(message.as_ref().as_bytes()).await?;
    writer.write_all("\n".as_bytes()).await?;

    Ok(())
}
