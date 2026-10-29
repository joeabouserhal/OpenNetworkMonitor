mod etw;
mod identity;
pub mod wifi;
mod windows;
pub use etw::EtwCapture;
pub use identity::process_labels;
pub use windows::{
    InstanceGuard, adapters, all_adapters, connections, historical_adapter_ids, process_names,
    visible_adapter_ids,
};
