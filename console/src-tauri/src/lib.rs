pub mod api_client;
pub mod commands;
pub mod error;
pub mod master_key;
pub mod settings;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![
            // Master key
            master_key::master_status,
            master_key::generate_master_key,
            // Legacy single-server settings (kept for backward compat)
            settings::get_server_url_cmd,
            settings::set_server_url_cmd,
            settings::get_server_ca_cmd,
            settings::set_server_ca_cmd,
            // Multi-server profile management
            settings::list_servers_cmd,
            settings::add_server_cmd,
            settings::remove_server_cmd,
            settings::set_active_server_cmd,
            settings::get_active_server_cmd,
            // Control plane
            commands::pair_server,
            commands::issue_machine,
            commands::list_machines,
            commands::machine_status,
            commands::machine_snapshots,
            // Shell / revoke / audit
            commands::set_shell,
            commands::run_shell,
            commands::revoke_machine,
            commands::audit,
        ])
        .run(tauri::generate_context!())
        .expect("运行 FleetWatch 管理端出错");
}
