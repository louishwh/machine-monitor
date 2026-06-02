pub mod error;
pub mod master_key;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![
            master_key::master_status,
            master_key::generate_master_key,
        ])
        .run(tauri::generate_context!())
        .expect("运行 FleetWatch 管理端出错");
}
