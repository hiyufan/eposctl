//! `suite`: the official EPOS Gaming Suite's service and app.

use std::io;

use eposctl::suite;

fn yes_no(b: bool) -> &'static str {
    if b { "yes" } else { "no" }
}

fn print_status() {
    let s = suite::status();
    if !s.installed {
        println!("the EPOS Gaming Suite is not installed");
        return;
    }
    println!("service   running: {}  starts with Windows: {}", yes_no(s.service_running), yes_no(s.service_autostart));
    println!("app       running: {}  starts with Windows: {}", yes_no(s.app_running), yes_no(s.app_autostart));
}

/// Show the state, or switch everything off (`Some(false)`) or on (`Some(true)`).
/// Without administrator rights, runs itself again elevated for the switch.
pub fn run(enable: Option<bool>) -> io::Result<()> {
    let Some(on) = enable else {
        print_status();
        return Ok(());
    };
    if !suite::status().installed {
        return Err(io::Error::new(io::ErrorKind::NotFound, "the EPOS Gaming Suite is not installed"));
    }
    if suite::is_elevated() {
        suite::set_enabled(on)?;
    } else {
        let exe = std::env::current_exe()?;
        let code = suite::run_elevated(&exe, if on { "suite on" } else { "suite off" })?;
        if code != 0 {
            return Err(io::Error::other(format!("switching failed in the elevated step (exit code {code})")));
        }
    }
    if !on {
        suite::quit_app();
    }
    print_status();
    Ok(())
}
