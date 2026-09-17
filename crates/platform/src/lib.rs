pub mod notification;
pub mod process;
#[cfg(unix)]
pub mod unix;
#[cfg(windows)]
pub mod windows;

pub use notification::send_system_notification;
pub use process::{kill_process_tree, CommandBuilder, PlatformProcess};
