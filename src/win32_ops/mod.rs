pub mod prereq;
pub mod process;
pub mod registry_clean;
pub mod shell;
pub mod uninstall;

#[cfg(test)]
mod tests;

pub use prereq::{
    compare_version_or_number, evaluate_prereq_condition, execute_prereq_command,
    expand_env_and_install_vars, get_pe_file_version, prompt_prereq_abort,
};
pub use process::{
    find_running_target_processes, is_file_locked, terminate_running_target_processes,
};
pub use registry_clean::clean_all_application_registry_residue;
pub use shell::{
    create_demo_app_directory, create_shell_shortcut, expand_install_path, get_clipboard_text,
    get_known_folder_path, pick_folder_dialog, pick_open_file_dialog, pick_save_exe_dialog,
    query_disk_space, run_demo_app_greeting, set_clipboard_text, shell_open, to_wide,
};
pub use uninstall::{
    clean_empty_subdirectories, detect_existing_installation, execute_uninstall_steps,
    is_safe_application_folder, register_uninstall_entry, remove_uninstall_entry,
    schedule_self_delete, write_uninstaller_files, ExistingInstallInfo,
};

#[cfg(test)]
pub(crate) use process::query_restart_manager_locking_processes;
#[cfg(test)]
pub(crate) use registry_clean::clean_audio_policy_config;
