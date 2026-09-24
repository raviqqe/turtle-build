macro_rules! debug {
    ($context:expr, $buffer:expr, $template:literal, $($value:expr),+) => {
        if $context.options().debug {
            $crate::run::log::log!($buffer, $template, $($value),+);
        }
    };
}

macro_rules! profile {
    ($context:expr, $buffer:expr, $template:literal, $($value:expr),+) => {
        if $context.options().profile {
            $crate::run::log::log!($buffer, $template, $($value),+);
        }
    };
}

macro_rules! log {
    ($buffer:expr, $template:literal, $($value:expr),+) => {
        $buffer.write_stderr(
            ("turtle: ".to_owned() + &format!($template, $($value),+)).as_bytes(),
        );
        $buffer.write_stderr("\n".as_bytes());
    };
}

pub(crate) use {debug, log, profile};
