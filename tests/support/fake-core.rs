// Native core/app stand-in. Never starts or connects to a real core.
use std::io::{self, BufRead};
fn main() {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if let Ok(log) = std::env::var("HN_COMMAND_LOG") {
        use std::io::Write;
        writeln!(std::fs::OpenOptions::new().create(true).append(true).open(log).unwrap(), "{args:?}").unwrap();
    }
    let session = std::env::var("HC_SESSION").unwrap_or_default();
    if let Ok(home) = std::env::var("HERDR_APP_HOME") {
        std::fs::write(std::path::Path::new(&home).join("last-core-session"), &session).unwrap();
    }
    if args.first().map(String::as_str) != Some("connect") {
        // Opt-in native entry -> APP_BIN -> real nvim -> SELF probe. No UI/core.
        if let Ok(script) = std::env::var("HNV_CONTEXT_PROBE") {
            let out = std::process::Command::new(std::env::var("HNV_PROBE_NVIM").unwrap())
                .args(["--headless", "--noplugin", "-u", "NONE", "-i", "NONE", "-l", &script])
                .output().unwrap();
            if !out.status.success() {
                eprintln!("{}", String::from_utf8_lossy(&out.stderr));
                std::process::exit(1);
            }
        }
        println!("{{}}"); return;
    }
    for (i, line) in io::stdin().lock().lines().enumerate() {
        let line = line.unwrap();
        let result = if line.contains("doc.get") {
            if session == "A" { r#"{"version":1,"value":{"spaces":[{"id":"sp","tabs":[{"id":"same"},{"id":"onlyA"}]}]}}"# }
            else { r#"{"version":1,"value":{"spaces":[]}}"# }
        } else { "{}" };
        println!("{{\"jsonrpc\":\"2.0\",\"id\":{},\"result\":{}}}", i+1, result);
    }
}
