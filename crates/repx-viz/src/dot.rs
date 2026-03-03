macro_rules! dot_write {
    ($dst:expr, $($arg:tt)*) => {
        {
            use std::fmt::Write as _;
            let _ = write!($dst, $($arg)*);
        }
    };
}

macro_rules! dot_writeln {
    ($dst:expr, $($arg:tt)*) => {
        {
            use std::fmt::Write as _;
            let _ = writeln!($dst, $($arg)*);
        }
    };
}
