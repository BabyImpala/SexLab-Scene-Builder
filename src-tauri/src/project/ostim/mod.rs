//! OStim Standalone import/export adapter around the SLSB `Package` IR.

pub mod convert;
pub mod events;
pub mod export;
pub mod mapping;
pub mod nemesis;

#[cfg(test)]
mod compare_tests;

pub use convert::import_ostim_scenes;
pub use export::write_ostim_pack;
