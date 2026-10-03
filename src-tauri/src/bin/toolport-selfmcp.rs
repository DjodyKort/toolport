fn main() {
    if let Err(error) = conduit_lib::plus::selfmcp::serve_stdio() {
        eprintln!("toolport-selfmcp: {error}");
        std::process::exit(1);
    }
}
