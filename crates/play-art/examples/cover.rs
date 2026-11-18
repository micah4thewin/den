//! Fetch one cover, the way Play does, and say what happened:
//!
//!     cargo run -p play-art --example cover -- Genesis "Sonic the Hedgehog (USA, Europe)"

fn main() {
    let mut args = std::env::args().skip(1);
    let system = args.next().unwrap_or_else(|| "Genesis".to_string());
    let title = args
        .next()
        .unwrap_or_else(|| "Sonic the Hedgehog".to_string());
    let Some(system) = play_ident::System::from_name(&system) else {
        eprintln!("no system called {system}");
        std::process::exit(2);
    };
    let Some(name) = play_art::cover_name(system, &[&title]) else {
        println!("no box art name fits {title:?}");
        return;
    };
    println!("name     {name}");
    match play_art::Fetcher::new(play_art::Sources::default()).fetch(system, &name) {
        Ok(Some(bytes)) => println!("fetched  {} bytes", bytes.len()),
        Ok(None) => println!("no source had it"),
        Err(e) => println!("offline  {e}"),
    }
}
