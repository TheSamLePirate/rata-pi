//! Modal-specific code extracted from `app/mod.rs` during V3.d.
//!
//! Each submodule owns one modal's full lifecycle: key handling, body
//! rendering, and (where applicable) the async dispatcher that fires the
//! user's choice as an RPC. `mod.rs` re-exports the public surface its
//! own code needs and delegates everything else.

pub(super) mod bodies;
pub(super) mod interview;
pub(super) mod settings;
