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
            // Task 8 — control plane
            commands::pair_server,
            commands::issue_machine,
            commands::list_machines,
            commands::machine_status,
            commands::machine_snapshots,
        ])
        .run(tauri::generate_context!())
        .expect("运行 FleetWatch 管理端出错");
}
