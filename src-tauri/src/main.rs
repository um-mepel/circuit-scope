#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use tauri::menu::{Menu, SubmenuBuilder};
use tauri::Emitter;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_fs::init())
        .setup(|app| {
            let handle = app.handle().clone();
            // On macOS the first submenu becomes the app-name menu; add it first so "File" is separate.
            let app_submenu = SubmenuBuilder::new(&handle, "Circuit Scope")
                .text("about", "About Circuit Scope")
                .separator()
                .quit()
                .build()?;
            let file_submenu = SubmenuBuilder::new(&handle, "File")
                .text("open_folder", "Open Folder...")
                .text("open_file", "Open File...")
                .text("save", "Save")
                .build()?;
            let menu = Menu::new(&handle)?;
            menu.append(&app_submenu)?;
            menu.append(&file_submenu)?;
            let _ = app.set_menu(menu);

            app.on_menu_event(move |app, event| {
                let id = event.id().as_ref();
                if id == "open_folder" {
                    let _ = app.emit("menu-open-folder", ());
                } else if id == "open_file" {
                    let _ = app.emit("menu-open-file", ());
                } else if id == "save" {
                    let _ = app.emit("menu-save", ());
                }
            });
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running Verilog IDE application");
}

