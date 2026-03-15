pub mod redact;

use std::env;
use std::net::TcpListener;
use std::path::PathBuf;

pub fn can_open_browser() -> bool {
    if env::var_os("BROWSER").is_some() {
        return true;
    }

    browser_command_candidates()
        .iter()
        .any(|candidate| command_on_path(candidate))
}

pub fn can_bind_localhost(host: &str) -> Result<(), String> {
    TcpListener::bind((host, 0))
        .map(|listener| drop(listener))
        .map_err(|error| error.to_string())
}

fn browser_command_candidates() -> &'static [&'static str] {
    if cfg!(target_os = "macos") {
        &["open"]
    } else if cfg!(target_os = "linux") {
        &[
            "xdg-open",
            "gio",
            "gnome-open",
            "kde-open",
            "kde-open5",
            "sensible-browser",
        ]
    } else {
        &[]
    }
}

fn command_on_path(command: &str) -> bool {
    if command.contains(std::path::MAIN_SEPARATOR) {
        return PathBuf::from(command).is_file();
    }

    env::var_os("PATH")
        .into_iter()
        .flat_map(|paths| env::split_paths(&paths).collect::<Vec<_>>())
        .map(|path| path.join(command))
        .any(|path| path.is_file())
}
