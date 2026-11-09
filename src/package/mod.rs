pub mod archive;
pub mod config;
pub mod extract;

#[cfg(test)]
mod tests;

pub use archive::*;
pub use config::*;
pub use extract::*;

pub const PACKAGE_MAGIC: &[u8; 8] = b"NYANPK02";
pub const FOOTER_SIZE: u64 = 40;
pub const UNINSTALL_DATA_FILE: &str = "unins000.dat";
pub const UNINSTALL_EXE_FILE: &str = "unins000.exe";
pub const PREREQ_VIRTUAL_PREFIX: &str = "__nyan_prereq__/";

pub fn format_bytes(bytes: u64) -> String {
    const KB: f64 = 1024.0;
    const MB: f64 = KB * 1024.0;
    const GB: f64 = MB * 1024.0;
    let b = bytes as f64;
    if b >= GB {
        format!("{:.2} GB", b / GB)
    } else if b >= MB {
        format!("{:.2} MB", b / MB)
    } else if b >= KB {
        format!("{:.1} KB", b / KB)
    } else {
        format!("{} B", bytes)
    }
}
