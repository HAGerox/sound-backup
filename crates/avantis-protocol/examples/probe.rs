//! On-site diagnostic.
//!
//! `cargo run -p avantis-protocol --example probe` finds consoles with Director's UDP finder.
//! `STAGE_BACKUP_TRACE=1 cargo run -p avantis-protocol --example probe -- <address>` connects
//! and lists the stored User Shows, printing each protocol step.
use avantis_protocol::{find_consoles, list_stored_shows, FinderRoute};
use std::{net::Ipv4Addr, time::Duration};

fn main() {
    let result = match std::env::args().nth(1) {
        None => find_consoles(
            &[FinderRoute::broadcast(Ipv4Addr::UNSPECIFIED, Ipv4Addr::BROADCAST)],
            Duration::from_secs(2),
        )
        .map(|consoles| {
            println!("{} console(s) answered", consoles.len());
            for console in consoles {
                println!("  {}  {}", console.address, console.name);
            }
        }),
        Some(address) => list_stored_shows(&address).map(|shows| {
            println!("{} stored User Show(s)", shows.len());
            for show in shows {
                println!("  {}", show.name);
            }
        }),
    };
    if let Err(error) = result {
        eprintln!("error: {error}");
        std::process::exit(1);
    }
}
