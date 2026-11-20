//! DWG/DXF-to-Smart-P&ID conversion and SPPID class/tag/publish bridging (`L3` + `L4`).
//!
//! Kept in its own module tree (`src/io/pid_convert/`) so recognition mapping,
//! tag pairing, and publish conversion evolve independently without colliding
//! with `src/io/pid/` or `src/io/pid_legend/`.

pub mod class_map;

pub use class_map::{
    audit_document_blocks, default_legend_class_map, BlockAuditSummary, BlockMatchKind,
    BlockOverrideRule, BlockResolution, IgnoreReason, LegendClassMap, LegendMapRules, MappedBlock,
};
