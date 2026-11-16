use conduit_lib::plus_adapter::HostAdapter;

fn main() {
    if let Err(error) = toolport_plus::selfmcp::serve_stdio(&HostAdapter) {
        eprintln!("toolport-selfmcp: {error}");
        std::process::exit(1);
    }
}
