//! Fork logic. Depends only on `toolport-plus-api`; upstream reaches it through
//! `seam.rs` in conduit and the binaries in `toolport-plus-bins` call it directly.

pub mod egress;
pub mod status;
