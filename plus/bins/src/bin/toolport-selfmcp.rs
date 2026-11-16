use conduit_lib::plus_adapter::HostAdapter;

fn main() {
    println!(
        "{}",
        toolport_plus::status::status("toolport-selfmcp", &HostAdapter)
    );
}
