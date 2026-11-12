use core::fmt;

use serde::Deserialize;

#[derive(Debug, Deserialize, Default, Clone, Copy)]
pub enum Unit {
    KB,
    #[default]
    MB,
    GB,
}

impl Unit {
    pub fn divisor(self) -> f64 {
        match self {
            Self::KB => 1024.0,
            Self::MB => 1024.0 * 1024.0,
            Self::GB => 1024.0 * 1024.0 * 1024.0,
        }
    }

    pub fn convert(self, bytes: u64) -> f64 {
        bytes as f64 / self.divisor()
    }
}

impl fmt::Display for Unit {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let value = match self {
            Self::KB => "KB",
            Self::MB => "MB",
            Self::GB => "GB",
        };

        write!(formatter, "{value}")
    }
}
