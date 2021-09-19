use std::io::{self, Write};
use std::sync::{Arc, Mutex};

pub mod prelude {
    pub use crate::log::{LogLevel, Logger};
    pub use crate::{debug, debug_res, error, error_res, info, info_res, log, warn, warn_res};
}

#[macro_export]
macro_rules! log {
    ($logger:expr, $level:expr, $($arg:tt)*) => {
        {
            let mut content = format_args!($($arg)*).to_string();
            content.push('\n');
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

pub struct Logger<W> {
    dest: Arc<Mutex<W>>,
    minimum_log_level: LogLevel,
}

impl<W> Logger<W> {
    pub fn new(dest: W, minimum_log_level: LogLevel) -> Self {
        Self {
            dest: Arc::new(Mutex::new(dest)),
            minimum_log_level,
        }
    }

    pub fn log<T>(&self, level: LogLevel, message: T) -> io::Result<()>
    where
        W: Write,
        T: std::fmt::Display,
    {
        if level >= self.minimum_log_level {
            let content = format!("[{}] {}", level.header(), message);
            let mut guard = self.dest.lock().unwrap();
            write!(&mut *guard, "{}", content)
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
        let logger = Logger::new(&mut dest, LogLevel::Error);

        error!(logger, "Bob: {}", "oops");

        assert_eq!("[Error] Bob: oops\n", dest.logs[0].as_str());
    }

    #[test]
    fn test_warn() {
        let mut dest = Dest::new();
        let logger = Logger::new(&mut dest, LogLevel::Warning);

        warn!(logger, "Alice: {}", "oops");

        assert_eq!("[Warn] Alice: oops\n", dest.logs[0].as_str());
    }

    #[test]
    fn test_warn_sink() {
        let mut dest = Dest::new();
        // Logger for error level
        let logger = Logger::new(&mut dest, LogLevel::Error);

        warn!(logger, "Alice: {}", "oops");

        assert!(dest.logs.is_empty());
    }

    #[test]
    fn test_info() {
        let mut dest = Dest::new();
        let logger = Logger::new(&mut dest, LogLevel::Info);

        info!(logger, "Alice: {}", "oops");

        assert_eq!("[Info] Alice: oops\n", dest.logs[0].as_str());
    }

    #[test]
    fn test_debug() {
        let mut dest = Dest::new();
        let logger = Logger::new(&mut dest, LogLevel::Debug);

        debug!(logger, "Alice: {}", "oops");

        assert_eq!("[Debug] Alice: oops\n", dest.logs[0].as_str());
    }
}
