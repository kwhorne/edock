//! Build tasks for eDock. Run with `cargo xtask <task>`.
//!
//!   guest    build edock-init for the guest and pack guest/out/initramfs.cpio
//!   kernel   build the guest kernel into guest/out/Image (needs a Docker engine)
//!   disk     make the empty data disk template guest/out/data.img (needs a Docker engine)
//!   host     build the `edock` binary and sign it with the virtualization entitlement
//!   bundle   host + target/edock-engine: the signed binary and the guest laid
//!            out as an app bundle's Contents (MacOS/edock, Resources/guest/…),
//!            for the desktop app to copy into its own bundle
//!   linux    `edock` and `edock-init` for a Linux host, in target/linux/release
//!            (built in a Linux container: needs a Docker engine)
//!   all      guest + host
//!
//! `--arch x86_64` (or `intel`) makes guest, kernel, host and bundle build for
//! Intel Macs instead: an x86-64 guest in guest/out/x86_64 and the host
//! binary for x86_64-apple-darwin.

use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

/// Which Mac a build is for. eDock ships one app per architecture rather
/// than a universal one: the bundled docker CLI, Compose and buildx are most
/// of the download, and a universal app would carry both builds of each.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Arch {
    Arm64,
    X86_64,
}

impl Arch {
    /// `--arch x86_64` (`amd64`, `intel`) anywhere after the task; arm64 otherwise.
    fn from_args() -> Arch {
        let args = std::env::args().skip(2).collect::<Vec<_>>();
        let intel = args.windows(2).any(|w| w[0] == "--arch" && matches!(w[1].as_str(), "x86_64" | "amd64" | "intel"));
        if intel { Arch::X86_64 } else { Arch::Arm64 }
    }

    fn guest_target(self) -> &'static str {
        match self {
            Arch::Arm64 => "aarch64-unknown-linux-musl",
            Arch::X86_64 => "x86_64-unknown-linux-musl",
        }
    }

    fn host_target(self) -> &'static str {
        match self {
            Arch::Arm64 => "aarch64-apple-darwin",
            Arch::X86_64 => "x86_64-apple-darwin",
        }
    }

    /// The kernel file in guest/out and in the app (crates/edock/src/guest.rs).
    fn kernel(self) -> &'static str {
        match self {
            Arch::Arm64 => "Image",
            Arch::X86_64 => "bzImage",
        }
    }

    /// guest/out for Apple Silicon, guest/out/x86_64 for Intel.
    fn guest_out(self, root: &Path) -> io::Result<PathBuf> {
        let out = match self {
            Arch::Arm64 => root.join("guest/out"),
            Arch::X86_64 => root.join("guest/out/x86_64"),
        };
        fs::create_dir_all(&out)?;
        Ok(out)
    }

    /// Where the host binary lands: target/release when it is this Mac's own
    /// architecture (so `cargo run` and the Apple Silicon build stay as they
    /// were), target/<triple>/release when cross-built.
    fn host_bin(self, root: &Path) -> PathBuf {
        if self.native() { root.join("target/release/edock") } else { root.join("target").join(self.host_target()).join("release/edock") }
    }

    fn native(self) -> bool {
        (self == Arch::X86_64) == cfg!(target_arch = "x86_64")
    }
}

fn main() -> ExitCode {
    let task = std::env::args().nth(1).unwrap_or_default();
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap().to_path_buf();
    let arch = Arch::from_args();
    let result = match task.as_str() {
        "guest" => guest(&root, arch),
        "kernel" => kernel(&root, arch),
        "disk" => disk(&root),
        "host" => host(&root, arch),
        "linux" => linux(&root),
        "bundle" => bundle(&root, arch),
        "all" => guest(&root, arch).and_then(|()| host(&root, arch)),
        _ => {
            eprintln!("usage: cargo xtask <guest|kernel|disk|host|bundle|linux|all> [--arch x86_64]");
            return ExitCode::FAILURE;
        }
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("xtask {task}: {e}");
            ExitCode::FAILURE
        }
    }
}

fn run(cmd: &mut Command) -> io::Result<()> {
    let status = cmd.status()?;
    if !status.success() {
        return Err(io::Error::other(format!("{cmd:?} failed: {status}")));
    }
    Ok(())
}

fn guest(root: &Path, arch: Arch) -> io::Result<()> {
    let target = arch.guest_target();
    // edock-init is pure Rust: rust-lld links it against Rust's own musl, as
    // .cargo/config.toml does for arm64. Set here, not there, so the Linux
    // and Windows engine builds (which link C) keep their own linker.
    run(Command::new(env!("CARGO"))
        .current_dir(root)
        .env("CARGO_TARGET_X86_64_UNKNOWN_LINUX_MUSL_LINKER", "rust-lld")
        .args(["build", "--release", "-p", "edock-init", "--target", target]))?;
    let init = fs::read(root.join("target").join(target).join("release/edock-init"))?;

    // Uncompressed on purpose: it is small, and the kernel skips decompression.
    let mut cpio = Cpio::default();
    for dir in ["dev", "proc", "sys", "run", "tmp", "etc"] {
        cpio.dir(dir);
    }
    // The kernel opens /dev/console for PID 1 before any filesystem is mounted.
    cpio.char_dev("dev/console", 0o600, 5, 1);
    cpio.file("init", 0o755, &init);
    let path = arch.guest_out(root)?.join("initramfs.cpio");
    fs::write(&path, cpio.finish())?;
    println!("{} ({} KiB)", path.display(), fs::metadata(&path)?.len() / 1024);
    Ok(())
}

/// The arm64 kernel builds in the cached tree in the `edock-kbuild` volume;
/// the x86-64 one in its own, `edock-kbuild-x86_64`, since both build in the
/// source tree. On Apple Silicon the x86-64 kernel is cross-compiled.
fn kernel(root: &Path, arch: Arch) -> io::Result<()> {
    let src = root.join("guest/kernel");
    let out = arch.guest_out(root)?;
    let (karch, cache) = match arch {
        Arch::Arm64 => ("KARCH=arm64", "edock-kbuild:/build"),
        Arch::X86_64 => ("KARCH=x86_64", "edock-kbuild-x86_64:/build"),
    };
    run(Command::new("docker").args([
        "run",
        "--rm",
        "-e",
        "DEBIAN_FRONTEND=noninteractive",
        "-e",
        karch,
        "-v",
        cache,
        "-v",
        &format!("{}:/src:ro", src.display()),
        "-v",
        &format!("{}:/out", out.display()),
        "debian:trixie",
        "sh",
        "/src/build.sh",
    ]))
}

/// A sparse 64 GiB ext4 image with lazily initialised inode tables: it uses a
/// few MB on the Mac until data is written. Each install copies it (an APFS
/// clone, so instant) into its data directory on first boot.
/// The same ext4 image serves both architectures.
fn disk(root: &Path) -> io::Result<()> {
    let out = Arch::Arm64.guest_out(root)?;
    run(Command::new("docker").args([
        "run",
        "--rm",
        "-v",
        &format!("{}:/out", out.display()),
        "debian:trixie",
        "sh",
        "-c",
        "set -e; apt-get update -qq >/dev/null && apt-get install -y -qq e2fsprogs >/dev/null; \
         rm -f /out/data.img; truncate -s 64G /out/data.img; \
         mkfs.ext4 -q -F -m 0 -L edock-data -E lazy_itable_init=1,lazy_journal_init=1 /out/data.img",
    ]))?;
    let meta = fs::metadata(out.join("data.img"))?;
    println!("{} ({} GiB sparse)", out.join("data.img").display(), meta.len() >> 30);
    Ok(())
}

fn host(root: &Path, arch: Arch) -> io::Result<()> {
    let mut build = Command::new(env!("CARGO"));
    build.current_dir(root).args(["build", "--release", "-p", "edock"]);
    if !arch.native() {
        build.args(["--target", arch.host_target()]);
    }
    run(&mut build)?;
    let bin = arch.host_bin(root);
    if cfg!(target_os = "macos") {
        // Virtualization.framework refuses to start a VM without this entitlement.
        run(Command::new("codesign").args(["--force", "--sign", "-", "--entitlements"])
            .arg(root.join("crates/edock/edock.entitlements"))
            .arg(&bin))?;
    }
    println!("{} ({} KiB)", bin.display(), fs::metadata(&bin)?.len() / 1024);
    Ok(())
}

/// The Linux host build: native, in a Rust container, so C dependencies
/// (ring) find a compiler. The cargo registry is cached in a volume.
/// `--arch amd64` builds x86-64 binaries (on Apple silicon the container
/// runs under Rosetta), for Linux PCs and WSL2 on Windows.
fn linux(root: &Path) -> io::Result<()> {
    let amd64 = std::env::args().skip(2).collect::<Vec<_>>().windows(2).any(|w| w[0] == "--arch" && (w[1] == "amd64" || w[1] == "x86_64"));
    linux_build(root, amd64)
}

fn linux_build(root: &Path, amd64: bool) -> io::Result<()> {
    let (platform, out, cache) = if amd64 { ("linux/amd64", "linux-x86_64", "edock-cargo-amd64") } else { ("linux/arm64", "linux", "edock-cargo") };
    run(Command::new("docker").args(["run", "--rm", "--platform", platform, "-v"]).arg(format!("{}:/src", root.display())).args([
        "-v",
        &format!("{cache}:/usr/local/cargo/registry"),
        "-w",
        "/src",
        "-e",
        &format!("CARGO_TARGET_DIR=/src/target/{out}"),
        // The repo's cross-linking setting is for the guest build from macOS.
        "-e",
        "CARGO_TARGET_AARCH64_UNKNOWN_LINUX_MUSL_LINKER=cc",
        "rust:1-alpine",
        "sh",
        "-c",
        "apk add -q musl-dev && cargo build --release -p edock -p edock-init",
    ]))?;
    for b in ["edock", "edock-init"] {
        let p = root.join("target").join(out).join("release").join(b);
        println!("{} ({} KiB)", p.display(), fs::metadata(&p)?.len() / 1024);
    }
    Ok(())
}

/// target/edock-engine (target/intel/edock-engine for Intel): what an app
/// bundle's `Contents` needs to run the engine, so the desktop app can copy
/// it in as is. The binary finds the guest at `../Resources/guest` from its
/// own folder (crates/edock/src/guest.rs, `guest_dir`), or wherever
/// `EDOCK_GUEST_DIR` says. The 64 GiB sparse disk template goes as a sparse
/// tarball, `data.img.tgz`, which the engine unpacks on first boot.
fn bundle(root: &Path, arch: Arch) -> io::Result<()> {
    host(root, arch)?;
    let out = root.join(if arch.native() { "target/edock-engine" } else { "target/intel/edock-engine" });
    let _ = fs::remove_dir_all(&out);
    let (macos, res) = (out.join("MacOS"), out.join("Resources"));
    for d in [&macos, &res.join("guest")] {
        fs::create_dir_all(d)?;
    }
    fs::copy(arch.host_bin(root), macos.join("edock"))?;
    fs::copy(root.join("LICENSE"), res.join("LICENSE"))?;
    fs::copy(root.join("NOTICE"), res.join("NOTICE"))?;
    fs::copy(root.join("guest/kernel/SOURCE_OFFER.md"), res.join("KERNEL_SOURCE_OFFER.md"))?;
    let guest = arch.guest_out(root)?;
    for f in [arch.kernel(), "initramfs.cpio"] {
        fs::copy(guest.join(f), res.join("guest").join(f))?;
    }
    run(Command::new("tar").args(["-cSzf"]).arg(res.join("guest/data.img.tgz")).arg("-C").arg(Arch::Arm64.guest_out(root)?).arg("data.img"))?;
    println!("{}", out.display());
    Ok(())
}

/// Minimal writer for the "newc" cpio format the kernel unpacks as initramfs.
#[derive(Default)]
struct Cpio {
    buf: Vec<u8>,
    ino: u32,
}

impl Cpio {
    fn dir(&mut self, name: &str) {
        self.entry(name, 0o040_755, 2, (0, 0), &[]);
    }

    fn char_dev(&mut self, name: &str, perm: u32, major: u32, minor: u32) {
        self.entry(name, 0o020_000 | perm, 1, (major, minor), &[]);
    }

    fn file(&mut self, name: &str, perm: u32, data: &[u8]) {
        self.entry(name, 0o100_000 | perm, 1, (0, 0), data);
    }

    fn entry(&mut self, name: &str, mode: u32, nlink: u32, rdev: (u32, u32), data: &[u8]) {
        self.ino += 1;
        let fields = [
            self.ino,
            mode,
            0, // uid
            0, // gid
            nlink,
            0, // mtime: fixed, so builds are reproducible
            data.len() as u32,
            0, // devmajor
            0, // devminor
            rdev.0,
            rdev.1,
            name.len() as u32 + 1,
            0, // check
        ];
        self.buf.extend_from_slice(b"070701");
        for f in fields {
            write!(self.buf, "{f:08x}").unwrap();
        }
        self.buf.extend_from_slice(name.as_bytes());
        self.buf.push(0);
        self.pad();
        self.buf.extend_from_slice(data);
        self.pad();
    }

    fn pad(&mut self) {
        while !self.buf.len().is_multiple_of(4) {
            self.buf.push(0);
        }
    }

    fn finish(mut self) -> Vec<u8> {
        self.ino = 0;
        self.entry("TRAILER!!!", 0, 1, (0, 0), &[]);
        self.buf
    }
}
