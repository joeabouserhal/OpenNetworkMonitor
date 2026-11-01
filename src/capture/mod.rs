mod etw;
mod identity;
mod instance;
pub mod wifi;
mod windows;
pub use etw::EtwCapture;
pub use identity::process_labels;
pub use instance::{InstanceGuard, spawn_quit_listener};
pub use windows::{
    adapters, all_adapters, connections, historical_adapter_ids, process_names, visible_adapter_ids,
};
