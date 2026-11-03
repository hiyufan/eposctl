//! Open-source control for EPOS gaming headsets, independent of the discontinued
//! EPOS Gaming Suite service.

#[cfg(not(windows))]
compile_error!("eposctl currently supports Windows only");

pub mod apo;
pub mod audio;
pub mod capricorn;
pub mod headset;
pub mod hid;
pub mod objdir;
pub mod presets;
pub mod report;
