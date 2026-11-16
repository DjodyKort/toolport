use conduit_lib::plus_adapter::HostAdapter;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    std::process::exit(toolport_plus::ctl::run(&HostAdapter, &args));
}
