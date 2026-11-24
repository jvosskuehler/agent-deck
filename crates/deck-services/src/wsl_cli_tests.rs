use std::sync::Mutex;

use super::*;

/// Antwortet mit festen Ausgaben und merkt sich die Aufrufe; startet nie `wsl.exe`.
struct FakeWsl {
    reply: Vec<(i32, &'static str)>,
    calls: Mutex<Vec<Vec<String>>>,
}

impl FakeWsl {
    fn new(reply: Vec<(i32, &'static str)>) -> FakeWsl {
        FakeWsl { reply, calls: Mutex::new(Vec::new()) }
    }
}

impl WslCli for FakeWsl {
    fn run(&self, args: &[String], _: Duration) -> Result<(i32, Vec<u8>), String> {
        let mut calls = self.calls.lock().unwrap();
        let (code, out) = *self.reply.get(calls.len()).unwrap_or(&(1, ""));
        calls.push(args.to_vec());
        Ok((code, out.as_bytes().to_vec()))
    }
}

#[test]
fn home_and_mount_root_come_from_one_call_into_the_distro() {
    let cli = FakeWsl::new(vec![(0, "/home/dev\n/mnt/c/\n")]);
    assert_eq!(probe(&cli, "Ubuntu"), Some(Wsl::new("Ubuntu", "/home/dev", "/mnt/")));
    let cli = FakeWsl::new(vec![(0, "/home/dev\n/c/\n")]);
    assert_eq!(probe(&cli, "Ubuntu").unwrap().mount_root, "/");
    assert_eq!(probe(&FakeWsl::new(vec![(1, "")]), "Ubuntu"), None, "Distro startet nicht");
}

#[test]
fn claude_counts_as_installed_only_with_a_path_from_the_login_shell() {
    let cli = FakeWsl::new(vec![(0, "bash: no job control in this shell\n/home/dev/.local/bin/claude\n")]);
    assert_eq!(claude_path(&cli, "Ubuntu").as_deref(), Some("/home/dev/.local/bin/claude"));
    assert_eq!(cli.calls.lock().unwrap()[0], ["-d", "Ubuntu", "-e", "bash", "-lic", "command -v claude"]);
    assert_eq!(claude_path(&FakeWsl::new(vec![(1, "")]), "Ubuntu"), None);
}
