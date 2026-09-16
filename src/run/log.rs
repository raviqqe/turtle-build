/// Logs a debug message.
macro_rules! debug {
    ($context:expr, $console:expr, $template:literal, $($value:expr),+) => {
        if $context.options().debug {
            $crate::run::log::log!($console, $template, $($value),+);
        }
    };
}

/// Logs a profile message.
macro_rules! profile {
    ($context:expr, $console:expr, $template:literal, $($value:expr),+) => {
        if $context.options().profile {
            $crate::run::log::log!($console, $template, $($value),+);
        }
    };
}

/// Logs a message.
macro_rules! log {
    ($console:expr, $template:literal, $($value:expr),+) => {
        $console.write_stderr(
            ("turtle: ".to_owned() + &format!($template, $($value),+)).as_bytes(),
        ).await?;
        $console.write_stderr("\n".as_bytes()).await?;
    };
}

pub(crate) use {debug, log, profile};
