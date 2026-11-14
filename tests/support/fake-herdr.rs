// Compiled by the Windows integration harness, never calls a real Herdr.
fn main() {
    let args: Vec<_> = std::env::args().skip(1).collect();
    let snapshot = std::env::current_exe().unwrap().parent().unwrap().parent().unwrap().join("snapshot.json");
    if args == ["api", "snapshot"] {
        if let Ok(text) = std::fs::read_to_string(snapshot) { println!("{text}"); return; }
    }
    eprintln!("fake herdr: unreachable");
    std::process::exit(1);
}
