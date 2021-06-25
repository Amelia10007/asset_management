use std::io::{self, Write};
use std::marker::PhantomData;

pub mod prelude {
    pub use crate::log::{LogLevel, Logger};
    pub use crate::{debug, debug_res, error, error_res, info, info_res, log, warn, warn_res};
}

#[macro_export]
macro_rules! log {
    ($logger:expr, $level:expr, $($arg:tt)*) => {
        {
            let content = format_args!($($arg)*).to_string();
            $logger.log($level, content)
        }
    };
}

#[macro_export]
macro_rules! error_res {
    ($logger:expr, $($arg:tt)*) => {
        log!($logger, crate::log::LogLevel::Error, $($arg)*)
    };
}

#[macro_export]
macro_rules! error {
    ($logger:expr, $($arg:tt)*) => {
        error_res!($logger, $($arg)*).unwrap()
    };
}

#[macro_export]
macro_rules! warn_res {
    ($logger:expr, $($arg:tt)*) => {
        log!($logger, crate::log::LogLevel::Warning, $($arg)*)
    };
}

#[macro_export]
macro_rules! warn {
    ($logger:expr, $($arg:tt)*) => {
        warn_res!($logger, $($arg)*).unwrap()
    };
}

#[macro_export]
macro_rules! info_res {
    ($logger:expr, $($arg:tt)*) => {
        log!($logger, crate::log::LogLevel::Info, $($arg)*)
    };
}

#[macro_export]
macro_rules! info {
    ($logger:expr, $($arg:tt)*) => {
        info_res!($logger, $($arg)*).unwrap()
    };
}

#[macro_export]
macro_rules! debug_res {
    ($logger:expr, $($arg:tt)*) => {
        log!($logger, crate::log::LogLevel::Debug, $($arg)*)
    };
}

#[macro_export]
macro_rules! debug {
    ($logger:expr, $($arg:tt)*) => {
        debug_res!($logger, $($arg)*).unwrap()
    };
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum LogLevel {
    Debug,
    Info,
    /// Occurs an error, but sustainable
    Warning,
    /// Occurs an unsustainable error
    Error,
}

impl LogLevel {
    pub const fn header(self) -> &'static str {
        match self {
            LogLevel::Debug => "Debug",
            LogLevel::Info => "Info",
            LogLevel::Warning => "Warn",
            LogLevel::Error => "Error",
        }
    }
}

pub struct Logger<'a, W> {
    dest: W,
    minimum_log_level: LogLevel,
    indent: usize,
    _phantom: PhantomData<&'a ()>,
}

impl<'a, W> Logger<'a, W> {
    pub const fn new(dest: W, minimum_log_level: LogLevel) -> Self {
        Self {
            dest,
            minimum_log_level,
            indent: 0,
            _phantom: PhantomData,
        }
    }

    pub fn child(&mut self) -> Logger<'_, &'_ mut W> {
        Logger {
            dest: &mut self.dest,
            minimum_log_level: self.minimum_log_level,
            indent: self.indent + 1,
            _phantom: PhantomData,
        }
    }

    pub fn log<T>(&mut self, level: LogLevel, message: T) -> io::Result<()>
    where
        W: Write,
        T: std::fmt::Display,
    {
        if level >= self.minimum_log_level {
            let indent = std::iter::repeat(' ')
                .take(self.indent * 2)
                .collect::<String>();
            let content = format!("{}[{}] {}", indent, level.header(), message);
            write!(self.dest, "{}", content)
        } else {
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Dest {
        logs: Vec<String>,
    }

    impl Dest {
        fn new() -> Self {
            Self { logs: vec![] }
        }
    }

    impl Write for Dest {
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            let str = std::str::from_utf8(buf).unwrap();
            self.logs.push(str.to_string());

            Ok(str.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn test_level_ord() {
        use super::LogLevel::*;

        assert!(Error > Warning);
        assert!(Warning > Info);
        assert!(Info > Debug);
    }

    #[test]
    fn test_error() {
        let mut dest = Dest::new();
        let mut logger = Logger::new(&mut dest, LogLevel::Error);

        error!(logger, "Bob: {}", "oops");

        assert_eq!("[Error] Bob: oops", dest.logs[0].as_str());
    }

    #[test]
    fn test_warn() {
        let mut dest = Dest::new();
        let mut logger = Logger::new(&mut dest, LogLevel::Warning);

        warn!(logger, "Alice: {}", "oops");

        assert_eq!("[Warn] Alice: oops", dest.logs[0].as_str());
    }

    #[test]
    fn test_warn_sink() {
        let mut dest = Dest::new();
        // Logger for error level
        let mut logger = Logger::new(&mut dest, LogLevel::Error);

        warn!(logger, "Alice: {}", "oops");

        assert!(dest.logs.is_empty());
    }

    #[test]
    fn test_info() {
        let mut dest = Dest::new();
        let mut logger = Logger::new(&mut dest, LogLevel::Info);

        info!(logger, "Alice: {}", "oops");

        assert_eq!("[Info] Alice: oops", dest.logs[0].as_str());
    }

    #[test]
    fn test_debug() {
        let mut dest = Dest::new();
        let mut logger = Logger::new(&mut dest, LogLevel::Debug);

        debug!(logger, "Alice: {}", "oops");

        assert_eq!("[Debug] Alice: oops", dest.logs[0].as_str());
    }

    #[test]
    fn test_children() {
        let mut dest = Dest::new();
        let mut logger = Logger::new(&mut dest, LogLevel::Info);
        info!(logger, "parent");

        let mut child = logger.child();
        warn!(child, "child");

        let mut grandchild = child.child();
        error!(grandchild, "grandchild");

        assert_eq!("[Info] parent", dest.logs[0].as_str());
        assert_eq!("  [Warn] child", dest.logs[1].as_str());
        assert_eq!("    [Error] grandchild", dest.logs[2].as_str());
    }
}
