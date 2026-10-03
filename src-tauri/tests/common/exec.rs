#![allow(dead_code)]

use std::io::Write;
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

const PROBE_ENV: &str = "TOOLPORT_TEST_EXEC_PROBE";
const PROBE_DEADLINE: Duration = Duration::from_secs(10);

/// Write `script` (shebang first, `/bin/sh` syntax) as an executable and return
/// only once the kernel will run it.
///
/// A file that was just written can still be open for writing in a child that
/// another test thread forked meanwhile, until that child reaches its own
/// `exec`; running it in that window fails with `ETXTBSY`. Spawn errors are
/// then misreported by callers as "not found". The script exits on its first
/// line when `PROBE_ENV` is set, so running it once here, retrying while the
/// file is busy, proves it is ready without side effects.
pub fn write_executable(path: &Path, script: &str) {
    let (shebang, rest) = script
        .split_once('\n')
        .filter(|(first, _)| first.starts_with("#!") && first.contains("sh"))
        .expect("a fixture script starts with a sh shebang line");
    let probed = format!("{shebang}\n[ -z \"${PROBE_ENV}\" ] || exit 0\n{rest}");
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o755)
        .open(path)
        .expect("create fixture script");
    file.write_all(probed.as_bytes())
        .expect("write fixture script");
    file.sync_all().expect("sync fixture script");
    drop(file);
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755))
        .expect("chmod fixture script");
    wait_until_executable(path);
}

fn wait_until_executable(path: &Path) {
    let deadline = Instant::now() + PROBE_DEADLINE;
    loop {
        let result = Command::new(path)
            .env(PROBE_ENV, "1")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
        match result {
            Err(e)
                if e.kind() == std::io::ErrorKind::ExecutableFileBusy
                    && Instant::now() < deadline =>
            {
                std::thread::sleep(Duration::from_millis(5));
            }
            _ => return,
        }
    }
}
