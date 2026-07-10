// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    // Force X11 backend — GTK3/Wayland causes "app not responding" freezes on Linux
    std::env::set_var("GDK_BACKEND", "x11");
    turbo_downloader_lib::run()
}
