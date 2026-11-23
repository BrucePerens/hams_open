//! Size probe: a tiny binary that pulls in the core so cross builds can record a realistic size.
use hams_net_core::siit::Block;

fn main() {
    let arg = std::env::args().nth(1).unwrap_or_else(|| "2001:db8:1::/48".into());
    match Block::parse(&arg) {
        Ok(block) => println!("{}", block.embed(7, std::net::Ipv4Addr::new(10, 1, 2, 3))),
        Err(e) => eprintln!("{e}"),
    }
}
