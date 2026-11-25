#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use sparrow_app::app;

pub fn main() {
    if std::env::var_os("SPARROW_GIT_ASKPASS_MODE").is_some() {
        std::process::exit(sparrow_app::vcs_credentials::askpass());
    }
    app::launch();
}
