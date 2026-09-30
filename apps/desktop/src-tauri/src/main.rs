#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    match std::env::args().nth(1).as_deref() {
        Some("--print-env") => return erindi_core::shell_env::print_env(),
        Some("--guard") => return erindi_desktop::guard::run(),
        _ => {}
    }
    erindi_desktop::run()
}
