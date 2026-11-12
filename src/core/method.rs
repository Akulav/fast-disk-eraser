//! Overwrite methods: which patterns are written, in which order.

use serde::{Deserialize, Serialize};

/// What one pass writes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pattern {
    Byte(u8),
    Random,
}

impl Pattern {
    pub fn label(self) -> String {
        match self {
            Pattern::Byte(b) => format!("0x{b:02X}"),
            Pattern::Random => "random".into(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum Method {
    /// One pass of zeros. Fastest; good for SSDs and modern drives.
    Zero,
    /// One pass of random data (default). As fast as zeros, leaves no recognisable pattern.
    #[default]
    Random,
    /// DoD 5220.22-M: 0x00, 0xFF, random.
    Dod3,
    /// DoD 5220.22-M ECE: the 3 DoD passes, random, then the 3 DoD passes again.
    Dod7,
    /// Bruce Schneier: 0xFF, 0x00, then five random passes.
    Schneier7,
}

impl Method {
    pub const ALL: [Method; 5] = [Method::Random, Method::Zero, Method::Dod3, Method::Dod7, Method::Schneier7];

    pub fn name(self) -> &'static str {
        match self {
            Method::Zero => "Quick zero",
            Method::Random => "Random",
            Method::Dod3 => "DoD 5220.22-M",
            Method::Dod7 => "DoD 5220.22-M ECE",
            Method::Schneier7 => "Schneier",
        }
    }

    pub fn description(self) -> &'static str {
        match self {
            Method::Zero => "One pass of zeros. The fastest option.",
            Method::Random => "One pass of random data. Recommended for almost everyone.",
            Method::Dod3 => "Zeros, ones, then random data. The classic US DoD standard.",
            Method::Dod7 => "The DoD passes twice with random data between them.",
            Method::Schneier7 => "Ones, zeros, then five passes of random data.",
        }
    }

    pub fn passes(self) -> Vec<Pattern> {
        use Pattern::*;
        match self {
            Method::Zero => vec![Byte(0x00)],
            Method::Random => vec![Random],
            Method::Dod3 => vec![Byte(0x00), Byte(0xFF), Random],
            Method::Dod7 => vec![Byte(0x00), Byte(0xFF), Random, Random, Byte(0x00), Byte(0xFF), Random],
            Method::Schneier7 => vec![Byte(0xFF), Byte(0x00), Random, Random, Random, Random, Random],
        }
    }

    pub fn pass_count(self) -> u32 {
        self.passes().len() as u32
    }

    /// Parse the CLI spelling (`zero`, `random`, `dod3`, `dod7`, `schneier`).
    pub fn parse(s: &str) -> Option<Self> {
        match s.to_ascii_lowercase().as_str() {
            "zero" | "zeros" | "quick" => Some(Method::Zero),
            "random" | "rand" => Some(Method::Random),
            "dod" | "dod3" => Some(Method::Dod3),
            "dod7" | "ece" => Some(Method::Dod7),
            "schneier" | "schneier7" => Some(Method::Schneier7),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pass_counts() {
        assert_eq!(Method::Zero.pass_count(), 1);
        assert_eq!(Method::Dod3.pass_count(), 3);
        assert_eq!(Method::Dod7.pass_count(), 7);
        assert_eq!(Method::Schneier7.pass_count(), 7);
        assert_eq!(Method::parse("DoD7"), Some(Method::Dod7));
        assert_eq!(Method::parse("nope"), None);
    }
}
