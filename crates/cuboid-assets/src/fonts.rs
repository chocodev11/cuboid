//! The embedded interface fonts (Inter 4.1, SIL OFL 1.1): one static file per weight, because egui
//! has no synthetic bold. Sources and checksums are in `THIRD_PARTY.md`.

/// Inter Regular, weight 400.
pub const INTER_REGULAR: &[u8] = include_bytes!("../fonts/inter/Inter-Regular.ttf");
/// Inter Medium, weight 500.
pub const INTER_MEDIUM: &[u8] = include_bytes!("../fonts/inter/Inter-Medium.ttf");
/// Inter SemiBold, weight 600.
pub const INTER_SEMIBOLD: &[u8] = include_bytes!("../fonts/inter/Inter-SemiBold.ttf");
