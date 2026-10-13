//! The root helper's version, shared by Settings and the helper. Raise it whenever
//! the helper's commands or behaviour change, so Settings offers to update the
//! installed copy; a rebuild with no change to the helper leaves it alone.

pub const HELPER_VERSION: u32 = 1;
