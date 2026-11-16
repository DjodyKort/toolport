use conduit_lib::plus_adapter::HostAdapter;
use toolport_plus_api::RegistryAccess;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = match args.iter().map(String::as_str).collect::<Vec<_>>()[..] {
        [] | ["status"] => {
            println!(
                "{}",
                toolport_plus::status::status("toolportctl", &HostAdapter)
            );
            Ok(())
        }
        ["set-server-enabled", id, enabled @ ("true" | "false")] => {
            HostAdapter.set_server_enabled(id, enabled == "true")
        }
        _ => Err("usage: toolportctl [status | set-server-enabled <id> <true|false>]".into()),
    };
    if let Err(error) = result {
        eprintln!("{error}");
        std::process::exit(2);
    }
}
