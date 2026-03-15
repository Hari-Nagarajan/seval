//! Color support detection from terminal environment variables.
//!
//! Checks `COLORTERM` and `TERM` to determine the highest supported color
//! level. This info can be used by themes and components to degrade gracefully.

/// Detected terminal color capability.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColorLevel {
    /// 24-bit / 16 million colors (truecolor).
    TrueColor,
    /// 256-color palette.
    Color256,
    /// Basic 16/8 colors.
    Basic,
}

impl std::fmt::Display for ColorLevel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::TrueColor => write!(f, "TrueColor (24-bit)"),
            Self::Color256 => write!(f, "256 colors"),
            Self::Basic => write!(f, "Basic (16 colors)"),
        }
    }
}

/// Detect the terminal color support level from environment variables.
///
/// Checks `COLORTERM` for truecolor/24bit indicators, then `TERM` for
/// 256color support, falling back to basic colors.
#[must_use]
pub fn detect_color_level() -> ColorLevel {
    // Check COLORTERM first — most reliable indicator of truecolor support.
    if let Ok(colorterm) = std::env::var("COLORTERM") {
        let ct = colorterm.to_lowercase();
        if ct.contains("truecolor") || ct.contains("24bit") {
            return ColorLevel::TrueColor;
        }
    }

    // Check TERM for 256-color support.
    if let Ok(term) = std::env::var("TERM")
        && term.contains("256color")
    {
        return ColorLevel::Color256;
    }

    ColorLevel::Basic
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_variants() {
        assert_eq!(format!("{}", ColorLevel::TrueColor), "TrueColor (24-bit)");
        assert_eq!(format!("{}", ColorLevel::Color256), "256 colors");
        assert_eq!(format!("{}", ColorLevel::Basic), "Basic (16 colors)");
    }
}
