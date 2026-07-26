//! OStim Standalone import/export adapter around the SLSB `Package` IR.

pub mod convert;
pub mod events;
pub mod export;
pub mod mapping;
pub mod nemesis;
pub mod nemesis_gen;

#[cfg(test)]
mod compare_tests;

pub use convert::import_ostim_scenes_with_progress;
pub use export::write_ostim_pack;
