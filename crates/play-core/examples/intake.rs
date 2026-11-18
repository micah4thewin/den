//! File a drop into a library from the command line, and print the report:
//!
//!     cargo run -p play-core --example intake -- <library> <file-or-folder>...

fn main() {
    let mut args = std::env::args().skip(1);
    let library = std::path::PathBuf::from(args.next().expect("a library folder"));
    let drops: Vec<std::path::PathBuf> = args.map(std::path::PathBuf::from).collect();
    let play = play_core::Play::open(&library).expect("open the library");
    let report = play.intake_many(&drops, None).expect("intake");
    for entry in &report.entries {
        println!("{:<12} {}", entry.outcome.word(), entry.input);
    }
}
