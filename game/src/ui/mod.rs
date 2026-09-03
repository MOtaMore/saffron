//! Player-facing UI: the HUD, the title / world / server / graphics menus, the
//! pause menu (and the `GameFlow` state), and the standalone structure editor.
//!
//! Re-exported flat at the crate root by `main.rs` (`crate::hud::…`,
//! `crate::menu::…`, `crate::pause::…`, `crate::editor::…`).

pub mod editor;
pub mod hud;
pub mod menu;
pub mod pause;
