#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    if std::env::args().nth(1).as_deref() == Some("--print-env") {
        return erindi_core::shell_env::print_env();
    }
    erindi_desktop::run()
}
