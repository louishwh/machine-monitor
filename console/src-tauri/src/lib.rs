pub mod api_client;
pub mod commands;
pub mod error;
pub mod master_key;
pub mod settings;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![
            // Task 7 — master key
            master_key::master_status,
            master_key::generate_master_key,
            // Task 8 — settings
            settings::get_server_url_cmd,
            settings::set_server_url_cmd,
            // M6 — server CA (self-signed TLS)
            settings::get_server_ca_cmd,
            settings::set_server_ca_cmd,
            // Task 8 — control plane
            commands::pair_server,
            commands::issue_machine,
            commands::list_machines,
            commands::machine_status,
            commands::machine_snapshots,
            // Task 5 — shell / revoke / audit
            commands::set_shell,
            commands::run_shell,
            commands::revoke_machine,
            commands::audit,
        ])
        .run(tauri::generate_context!())
        .expect("运行 FleetWatch 管理端出错");
}
