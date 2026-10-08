//! `trackpad-bridge`: flutter_rust_bridge surface.
//!
//! Phase 4: real FRB API in [`api`]. The phone uses the exact same
//! `trackpad-core` as the desktop. Run codegen with:
//! `flutter_rust_bridge_codegen generate` (see `flutter_rust_bridge.yaml`).

mod frb_generated; /* AUTO INJECTED BY flutter_rust_bridge. This line may not be accurate, and you can change it according to your needs. */

pub mod api;

/// Phase 0 smoke string (kept for history).
pub fn bridge_hello() -> String {
    api::bridge_hello()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bridge_links_core() {
        assert!(bridge_hello().contains("trackpad-core hello"));
    }
}
