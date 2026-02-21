//! LSP request/notification handlers.

mod code_action;
mod documents;
mod files;
mod initialize;
mod symbols;

pub use code_action::handle_code_action;
pub use documents::{handle_did_change, handle_did_close, handle_did_open, handle_did_save};
pub use files::handle_did_change_watched_files;
pub use initialize::{handle_initialize, handle_initialized, handle_shutdown};
pub use symbols::handle_document_symbol;
