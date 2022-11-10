#[macro_export]
macro_rules! debug {
    ($context:expr, $console:expr, $template:literal, $($value:expr),+) => {
        if $context.options().debug {
            $crate::log!($console, $template, $($value),+);
        }
    };
}

#[macro_export]
macro_rules! profile {
    ($context:expr, $console:expr, $template:literal, $($value:expr),+) => {
        if $context.options().profile {
            $crate::log!($console, $template, $($value),+);
        }
    };
}

#[macro_export]
macro_rules! log {
    ($console:expr, $template:literal, $($value:expr),+) => {
        $console.write_stderr(
            ("turtle: ".to_owned() + &format!($template, $($value),+)).as_bytes(),
        ).await?;
        $console.write_stderr("\n".as_bytes()).await?;
    };
}
