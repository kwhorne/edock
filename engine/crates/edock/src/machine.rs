//! `edock machine`: long-lived Linux machines, as OrbStack has. A machine is
//! a container that keeps running, from a distro image, with the host user's
//! name and uid inside (and passwordless sudo), and the homes folder
//! (`/Users` on a Mac, `/home` on Linux) at the same path as on the host.
//! `edock machine shell` opens a login shell in it.
//!
//! `ssh NAME.edock` reaches a machine too, for editors that work over SSH
//! (VS Code, Cursor, Zed): `edock machine ssh on` adds a `Host *.edock`
//! block (in `~/.edock/ssh/config`, included from `~/.ssh/config`) whose
//! ProxyCommand is `edock machine stdio NAME`. That runs the machine's own
//! `sshd -i` over the engine's exec API, with eDock's key: no daemon, no
//! port, no password. openssh-server is installed in the machine the first
//! time.

use std::io;
use std::process::Command;

use serde_json::json;

use crate::client;

const LABEL: &str = "app.edock.machine";

/// `edock machine create NAME --distro ubuntu` picks one of these.
const DISTROS: [(&str, &str); 7] = [
    ("ubuntu", "ubuntu:24.04"),
    ("debian", "debian:trixie"),
    ("alpine", "alpine:3.22"),
    ("fedora", "fedora:42"),
    ("arch", "archlinux:latest"),
    ("rocky", "rockylinux:9"),
    ("opensuse", "opensuse/tumbleweed:latest"),
];

fn container(name: &str) -> String {
    format!("machine-{name}")
}

fn ok(r: io::Result<client::Response>) -> io::Result<client::Response> {
    let r = r?;
    if r.ok() { Ok(r) } else { Err(io::Error::other(r.error())) }
}

pub fn run(args: &[String]) -> io::Result<()> {
    if !client::alive() {
        return Err(io::Error::other("edock daemon is not running: open eDock.app or run `edock daemon`"));
    }
    let name = args.get(1).map(String::as_str).unwrap_or_default();
    let need_name = || if name.is_empty() { Err(io::Error::other("name a machine")) } else { Ok(()) };
    match args.first().map(String::as_str) {
        Some("create") => {
            need_name()?;
            let pick = args.iter().position(|a| a == "--distro" || a == "--image").and_then(|i| args.get(i + 1)).map_or("ubuntu", String::as_str);
            create(name, pick, args.iter().any(|a| a == "--systemd"), args.iter().any(|a| a == "--ssh-agent"))
        }
        Some("list" | "ls") => list(),
        Some("shell") => {
            need_name()?;
            let cmd: Vec<&str> = args.iter().position(|a| a == "--").map(|i| args[i + 1..].iter().map(String::as_str).collect()).unwrap_or_default();
            shell(name, &cmd)
        }
        Some(verb @ ("start" | "stop" | "restart")) => {
            need_name()?;
            ok(client::request("POST", &format!("/containers/{}/{verb}", container(name)), None))?;
            Ok(())
        }
        Some("stdio") => {
            need_name()?;
            stdio(name)
        }
        Some("ssh") => ssh(args.get(1).map_or("status", String::as_str)),
        Some("delete" | "rm") => {
            need_name()?;
            ok(client::request("DELETE", &format!("/containers/{}?force=1&v=1", container(name)), None))?;
            println!("deleted machine {name}");
            Ok(())
        }
        _ => Err(io::Error::other(
            "usage: edock machine create NAME [--distro ubuntu|debian|alpine|fedora|arch|rocky|opensuse | --image IMAGE] [--systemd] [--ssh-agent]\n       edock machine list | shell NAME [-- CMD...] | start | stop | restart | delete NAME\n       edock machine ssh on | off | status   `ssh NAME.edock` for every machine\n       edock machine stdio NAME   (ssh's ProxyCommand)",
        )),
    }
}

/// The host's homes folder, mounted at the same path in every machine.
const HOMES: &str = if cfg!(target_os = "macos") { "/Users" } else { "/home" };

/// The host user's name, uid and gid. root is always `root`: when USER
/// isn't set, a made-up name with uid 0 was a user no machine had
/// ("unknown user user").
fn user() -> (String, u32, u32) {
    // SAFETY: getuid/getgid cannot fail.
    let (uid, gid) = unsafe { (libc::getuid(), libc::getgid()) };
    let name = if uid == 0 { "root".to_owned() } else { std::env::var("USER").ok().filter(|u| !u.is_empty()).unwrap_or_else(|| "user".to_owned()) };
    (name, uid, gid)
}

/// The user's home in the machine: under HOMES, as on the host; root's is /root.
fn home(user: &str) -> String {
    if user == "root" { "/root".to_owned() } else { format!("{HOMES}/{user}") }
}

/// Where a machine reaches the Mac's SSH agent: the engine relays it there
/// (Docker Desktop's and OrbStack's path for it).
const SSH_AGENT: &str = "/run/host-services/ssh-auth.sock";

fn create(name: &str, pick: &str, systemd: bool, ssh_agent: bool) -> io::Result<()> {
    if !name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-') || name.starts_with('-') {
        return Err(io::Error::other("a machine name is letters, digits and dashes"));
    }
    let image = DISTROS.iter().find(|(d, _)| *d == pick).map_or(pick, |(_, i)| *i);
    let (repo, tag) = match image.rsplit_once(':') {
        Some((r, t)) if !t.contains('/') => (r, t),
        _ => (image, "latest"),
    };
    println!("pulling {repo}:{tag}…");
    client::pull_or_keep(repo, tag)?;
    let (user, uid, gid) = user();
    let home = home(&user);
    // The host user, made once; every distro has one of these tools. An
    // image user that already has the uid (Ubuntu's `ubuntu` is 1000, the
    // first user's uid on most Linux hosts) is renamed to the host user.
    let init = format!(
        r#"set -e
if ! id -u {user} >/dev/null 2>&1; then
  getent group {gid} >/dev/null 2>&1 || groupadd -g {gid} {user} 2>/dev/null || addgroup -g {gid} {user} 2>/dev/null || true
  g=$(getent group {gid} | cut -d: -f1)
  old=$(getent passwd {uid} | cut -d: -f1)
  {{ [ -n "$old" ] && usermod -l {user} -d {home} "$old" 2>/dev/null; }} || useradd -o -M -u {uid} -g "${{g:-{gid}}}" -d {home} -s /bin/sh {user} 2>/dev/null || adduser -D -H -u {uid} -G "${{g:-{user}}}" -h {home} {user} 2>/dev/null || true
fi
mkdir -p /etc/sudoers.d && printf '%s\n' '{user} ALL=(ALL) NOPASSWD:ALL' 'Defaults env_keep += "SSH_AUTH_SOCK"' > /etc/sudoers.d/edock && chmod 440 /etc/sudoers.d/edock
# Slim images have no sudo: fetch it once, in the background.
{ssh}command -v sudo >/dev/null 2>&1 || (apt-get update -qq && DEBIAN_FRONTEND=noninteractive apt-get install -y -qq sudo || apk add -q sudo || dnf -y -q install sudo || pacman -Sy --noconfirm sudo || zypper -n -q in sudo) >/dev/null 2>&1 &
{tail}"#,
        // The Mac's SSH agent for login shells too (sudo -i, a new session).
        ssh = if ssh_agent { format!("mkdir -p /etc/profile.d && echo 'export SSH_AUTH_SOCK={SSH_AGENT}' > /etc/profile.d/edock-ssh-agent.sh\n") } else { String::new() },
        tail = if systemd {
            // A full init: systemd as PID 1 (installed first if the image lacks it).
            "wait\nfor i in /lib/systemd/systemd /usr/lib/systemd/systemd; do [ -x $i ] && exec $i; done\n(apt-get update -qq && DEBIAN_FRONTEND=noninteractive apt-get install -y -qq systemd systemd-sysv dbus || dnf -y -q install systemd || pacman -Sy --noconfirm systemd || zypper -n -q in systemd) >/dev/null 2>&1\nfor i in /lib/systemd/systemd /usr/lib/systemd/systemd; do [ -x $i ] && exec $i; done\necho 'no systemd for this distro' >&2; exit 1"
        } else {
            "trap 'exit 0' TERM INT\nwhile :; do sleep 3600 & wait $!; done"
        }
    );
    let mut body = json!({
        "Image": format!("{repo}:{tag}"),
        "Cmd": ["/bin/sh", "-c", init],
        "Hostname": name,
        "Labels": {LABEL: name},
        "HostConfig": {
            "Binds": [format!("{HOMES}:{HOMES}")],
            "RestartPolicy": {"Name": "unless-stopped"},
            "CapAdd": ["NET_ADMIN", "SYS_PTRACE"],
        },
    });
    if ssh_agent {
        body["HostConfig"]["Binds"].as_array_mut().expect("binds").push(format!("{SSH_AGENT}:{SSH_AGENT}").into());
        body["Env"] = json!([format!("SSH_AUTH_SOCK={SSH_AGENT}")]);
    }
    if systemd {
        // systemd needs its own cgroup tree, tmpfs /run and /tmp, and stops
        // on SIGRTMIN+3.
        body["StopSignal"] = "SIGRTMIN+3".into();
        let mut env = vec![json!("container=docker")];
        env.extend(body["Env"].as_array().cloned().unwrap_or_default());
        body["Env"] = env.into();
        body["HostConfig"]["Privileged"] = true.into();
        body["HostConfig"]["Tmpfs"] = json!({"/run": "", "/run/lock": "", "/tmp": ""});
    }
    ok(client::request("POST", &format!("/containers/create?name={}", container(name)), Some(&body)))?;
    ok(client::request("POST", &format!("/containers/{}/start", container(name)), None))?;
    // The user is made by the machine's own first command: wait for it, or
    // a `machine shell` straight after was "unknown user" (seen on Fedora).
    let wait = format!("for i in $(seq 300); do id -u {user} >/dev/null 2>&1 && exit 0; sleep 0.1; done; exit 1");
    if client::exec(&container(name), &["sh", "-c", &wait])?.0 != 0 {
        return Err(io::Error::other(format!("machine {name} is running, but the user {user} could not be added to it")));
    }
    println!("machine {name} ({image}) is running: edock machine shell {name}");
    Ok(())
}

fn list() -> io::Result<()> {
    let filters = format!("{{\"label\":[\"{LABEL}\"]}}");
    let rows = client::get(&format!("/containers/json?all=1&filters={}", urlish(&filters)))?;
    println!("{:<16} {:<28} {}", "NAME", "IMAGE", "STATE");
    for c in rows.as_array().into_iter().flatten() {
        println!("{:<16} {:<28} {}", c["Labels"][LABEL].as_str().unwrap_or("?"), c["Image"].as_str().unwrap_or(""), c["Status"].as_str().unwrap_or(""));
    }
    Ok(())
}

fn urlish(s: &str) -> String {
    s.bytes().map(|b| if b.is_ascii_alphanumeric() || b"-._~".contains(&b) { (b as char).to_string() } else { format!("%{b:02X}") }).collect()
}

/// A login shell as the host user, in the same directory as on the host when
/// it is under HOMES, else in the user's home. The docker CLI handles the
/// terminal.
fn shell(name: &str, cmd: &[&str]) -> io::Result<()> {
    let docker = client::docker_cli().ok_or_else(|| io::Error::other("no docker CLI found: run `edock setup` (Engine › Command line › Set up in the app)"))?;
    let (user, _, _) = user();
    let home = home(&user);
    let cwd = std::env::current_dir().ok().filter(|d| d.starts_with(HOMES)).map_or_else(|| home.clone(), |d| d.display().to_string());
    let mut c = Command::new(docker);
    // The engine the other machine commands use (client::socket(), so
    // EDOCK_SOCKET too), as `edock exec` does, not a docker context: with
    // `--context edock`, a second eDock's machines (or any, once that context
    // was removed or pointed elsewhere) were "No such container".
    c.env("DOCKER_HOST", format!("unix://{}", client::socket()?.display())).env_remove("DOCKER_CONTEXT");
    // SAFETY: isatty has no preconditions.
    let tty = unsafe { libc::isatty(0) } == 1;
    c.args(["exec", if tty { "-it" } else { "-i" }, "-u", &user, "-w", &cwd, "-e", &format!("HOME={home}"), &container(name)]);
    if cmd.is_empty() {
        // bash when the distro has it, else its sh.
        c.args(["sh", "-c", "exec $(command -v bash || command -v sh) -l"]);
    } else {
        c.args(cmd);
    }
    let status = c.status()?;
    if !status.success() {
        std::process::exit(status.code().unwrap_or(1));
    }
    Ok(())
}

// ── SSH ────────────────────────────────────────────────────────────────

/// Installs openssh-server with the distro's own package manager, once
/// another one has finished (a new machine fetches sudo in the background,
/// holding apt's lock). One at a time: VS Code and Cursor open several
/// connections at once, and a second waits for the first's install. Its
/// last lines when it fails.
const INSTALL_SSHD: &str = r#"for i in $(seq 300); do mkdir /run/edock-sshd.lock 2>/dev/null && break; [ -x /usr/sbin/sshd ] && exit 0; sleep 1; done
trap 'rmdir /run/edock-sshd.lock' EXIT
[ -x /usr/sbin/sshd ] && exit 0
busy() { grep -qsxE 'apt-get|apt|dpkg|apk|dnf|rpm|pacman|zypper' /proc/[0-9]*/comm; }
for i in $(seq 180); do busy || break; sleep 1; done
log=/tmp/edock-ssh-install.log
if command -v apt-get >/dev/null; then
  { apt-get update -qq && DEBIAN_FRONTEND=noninteractive apt-get install -y -qq openssh-server; } >$log 2>&1
elif command -v apk >/dev/null; then apk add -q openssh >$log 2>&1
elif command -v dnf >/dev/null; then dnf -y -q install openssh-server >$log 2>&1
elif command -v pacman >/dev/null; then pacman -Sy --noconfirm openssh >$log 2>&1
elif command -v zypper >/dev/null; then zypper -n -q in openssh >$log 2>&1
else echo "no package manager eDock knows"; exit 1
fi
c=$?; [ $c -eq 0 ] || tail -3 $log; exit $c"#;

/// eDock's own SSH files: its key, and the `Host *.edock` block.
fn ssh_dir() -> io::Result<std::path::PathBuf> {
    let home = std::env::var_os("HOME").ok_or_else(|| io::Error::other("HOME is not set"))?;
    Ok(std::path::PathBuf::from(home).join(crate::flavor::STATE.get()).join("ssh"))
}

/// The line that brings eDock's block into `~/.ssh/config`. First in the
/// file: an Include after a `Host` line would only apply to that host.
fn include_line() -> String {
    format!("Include ~/{}/ssh/config", crate::flavor::STATE.get())
}

fn user_ssh_config() -> io::Result<std::path::PathBuf> {
    let home = std::env::var_os("HOME").ok_or_else(|| io::Error::other("HOME is not set"))?;
    Ok(std::path::PathBuf::from(home).join(".ssh/config"))
}

/// The `Host *.edock` block: this `edock` as the ProxyCommand, eDock's key
/// and no agent (a `Host *` IdentityAgent, such as 1Password's, would be
/// asked on every connection), and no host-key checks (the way in is the
/// engine's socket, which only this user can open, and a machine made again
/// has new host keys).
fn host_block(edock: &std::path::Path, user: &str, key: &std::path::Path) -> String {
    format!(
        "# Written by eDock (`edock machine ssh on`): `ssh NAME.edock` reaches the machine NAME.\n\
         Host *.edock\n  User {user}\n  ProxyCommand \"{}\" machine stdio %n\n  IdentityFile \"{}\"\n  IdentitiesOnly yes\n  IdentityAgent none\n  StrictHostKeyChecking no\n  UserKnownHostsFile /dev/null\n  LogLevel ERROR\n",
        edock.display(),
        key.display()
    )
}

/// `~/.ssh/config` with eDock's Include first, or without it.
fn with_include(config: &str, on: bool) -> String {
    let line = include_line();
    let rest: Vec<&str> = config.lines().filter(|l| l.trim() != line).collect();
    let rest = rest.join("\n");
    let rest = rest.trim_start_matches('\n');
    match (on, rest.is_empty()) {
        (true, true) => format!("{line}\n"),
        (true, false) => format!("{line}\n\n{rest}\n"),
        (false, true) => String::new(),
        (false, false) => format!("{rest}\n"),
    }
}

fn ssh(what: &str) -> io::Result<()> {
    let dir = ssh_dir()?;
    let key = dir.join("id_ed25519");
    let config = user_ssh_config()?;
    let current = std::fs::read_to_string(&config).unwrap_or_default();
    let on = current.lines().any(|l| l.trim() == include_line()) && dir.join("config").is_file();
    match what {
        "status" => {
            println!("{}", if on { "on" } else { "off" });
            Ok(())
        }
        "on" => {
            // ssh refuses an included config others may write (with a umask
            // of 002, every ssh and git command on the Mac would fail).
            std::fs::create_dir_all(&dir)?;
            {
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700))?;
            }
            if !key.is_file() {
                let made = Command::new("/usr/bin/ssh-keygen").args(["-q", "-t", "ed25519", "-N", "", "-C", "edock"]).arg("-f").arg(&key).status()?;
                if !made.success() {
                    return Err(io::Error::other("ssh-keygen couldn't make eDock's key"));
                }
            }
            let me = std::env::current_exe()?.canonicalize()?;
            write_private(&dir.join("config"), &host_block(&me, &user().0, &key))?;
            if let Some(parent) = config.parent() {
                std::fs::create_dir_all(parent)?;
                use std::os::unix::fs::PermissionsExt;
                let _ = std::fs::set_permissions(parent, std::fs::Permissions::from_mode(0o700));
            }
            write_private(&config, &with_include(&current, true))?;
            println!("on: `ssh NAME.edock` reaches the machine NAME (VS Code, Cursor and Zed too)");
            Ok(())
        }
        "off" => {
            if config.is_file() {
                write_private(&config, &with_include(&current, false))?;
            }
            let _ = std::fs::remove_file(dir.join("config"));
            println!("off: ~/.ssh/config no longer includes eDock's");
            Ok(())
        }
        _ => Err(io::Error::other("usage: edock machine ssh on | off | status")),
    }
}

/// Writes a file only this user can read or write, as ssh wants its config.
fn write_private(path: &std::path::Path, text: &str) -> io::Result<()> {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;
    let tmp = path.with_extension("edock-tmp");
    let mut f = std::fs::OpenOptions::new().write(true).create(true).truncate(true).mode(0o600).open(&tmp)?;
    f.write_all(text.as_bytes())?;
    drop(f);
    std::fs::rename(&tmp, path)
}

/// `edock machine stdio NAME`: ssh's ProxyCommand. The machine's `sshd -i`
/// on this process's stdin and stdout, as root through the exec API, with
/// eDock's key as the one that may log in. Starts the machine when it's
/// stopped, and installs openssh-server in it the first time.
fn stdio(name: &str) -> io::Result<()> {
    let name = name.strip_suffix(".edock").unwrap_or(name);
    if name.is_empty() || !name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-') || name.starts_with('-') {
        return Err(io::Error::other(format!("{name:?} isn't a machine's name")));
    }
    let id = container(name);
    let info = client::get(&format!("/containers/{id}/json")).map_err(|_| io::Error::other(format!("there's no machine {name}: `edock machine list`")))?;
    if info["Config"]["Labels"][LABEL].as_str() != Some(name) {
        return Err(io::Error::other(format!("there's no machine {name}: `edock machine list`")));
    }
    if info["State"]["Running"] != true {
        eprintln!("starting {name}…");
        ok(client::request("POST", &format!("/containers/{id}/start"), None))?;
    }
    let pubkey = std::fs::read_to_string(ssh_dir()?.join("id_ed25519.pub")).map_err(|_| io::Error::other("eDock's SSH key isn't there: `edock machine ssh on`"))?;
    let (user, _, _) = user();
    if client::exec(&id, &["test", "-x", "/usr/sbin/sshd"])?.0 != 0 {
        eprintln!("installing openssh-server in {name} (once)…");
        let (code, out) = client::exec(&id, &["sh", "-c", INSTALL_SSHD])?;
        if code != 0 {
            return Err(io::Error::other(format!("couldn't install openssh-server in {name}: {}", out.trim())));
        }
    }
    // Host keys, the run directory sshd wants, eDock's key, and the user
    // unlocked for key logins (useradd leaves `!`, which sshd refuses
    // without PAM). Host keys made one connection at a time, and the key
    // file replaced whole: another connection's sshd may be reading it.
    let prepare = format!(
        "set -e; mkdir -p /etc/edock /run/sshd; chmod 755 /run/sshd; \
         for i in $(seq 100); do mkdir /run/edock-sshd.lock 2>/dev/null && break; sleep 0.1; done; \
         trap 'rmdir /run/edock-sshd.lock 2>/dev/null' EXIT; ssh-keygen -A >/dev/null 2>&1; \
         printf '%s\\n' \"$1\" > /etc/edock/.authorized_keys.$$; chmod 644 /etc/edock/.authorized_keys.$$; \
         mv -f /etc/edock/.authorized_keys.$$ /etc/edock/authorized_keys; \
         sed -i 's/^{user}:![^:]*:/{user}:*:/' /etc/shadow 2>/dev/null || true"
    );
    let (code, out) = client::exec(&id, &["sh", "-c", &prepare, "sh", pubkey.trim()])?;
    if code != 0 {
        return Err(io::Error::other(format!("couldn't make {name} ready for ssh: {}", out.trim())));
    }
    let docker = client::docker_cli().ok_or_else(|| io::Error::other("no docker CLI found: run `edock setup` (Engine › Command line › Set up in the app)"))?;
    let mut c = Command::new(docker);
    c.env("DOCKER_HOST", format!("unix://{}", client::socket()?.display())).env_remove("DOCKER_CONTEXT");
    c.args(["exec", "-i", &id, "/usr/sbin/sshd", "-i", "-e"]);
    // Forwarding on whatever the distro's sshd_config says (Alpine's is
    // off): VS Code's Remote-SSH and `ssh -L` need it.
    for o in ["AuthorizedKeysFile=/etc/edock/authorized_keys .ssh/authorized_keys", "PasswordAuthentication=no", "KbdInteractiveAuthentication=no", "PermitRootLogin=no", "AllowTcpForwarding=yes", "LogLevel=ERROR"] {
        c.args(["-o", o]);
    }
    use std::os::unix::process::CommandExt;
    // This process becomes the docker CLI: ssh's pipes go straight through.
    Err(c.exec())
}

#[cfg(test)]
mod ssh_tests {
    use super::*;

    #[test]
    fn the_include_goes_first_once_and_comes_out_cleanly() {
        let line = include_line();
        assert_eq!(with_include("", true), format!("{line}\n"));
        let mine = "Host gh\n  HostName github.com\n";
        let on = with_include(mine, true);
        assert!(on.starts_with(&format!("{line}\n\n")), "{on}");
        assert!(on.ends_with(mine), "{on}");
        assert_eq!(with_include(&on, true), on, "once");
        assert_eq!(with_include(&on, false), mine);
        assert_eq!(with_include(&format!("{line}\n"), false), "");
    }

    #[test]
    fn the_block_uses_this_edock_and_the_key() {
        let b = host_block(std::path::Path::new("/Applications/eDock.app/Contents/MacOS/edock"), "kh", std::path::Path::new("/Users/kh/.edock/ssh/id_ed25519"));
        assert!(b.contains("Host *.edock\n"));
        assert!(b.contains("ProxyCommand \"/Applications/eDock.app/Contents/MacOS/edock\" machine stdio %n"));
        assert!(b.contains("IdentityFile \"/Users/kh/.edock/ssh/id_ed25519\""));
        assert!(b.contains("User kh"));
        // Not a `Host *` agent (1Password's asks for Touch ID on each one).
        assert!(b.contains("IdentitiesOnly yes\n  IdentityAgent none\n"));
    }
}
