//! One executable provides deployment and diagnostics. No scripting runtime.
mod boot;
mod install;
mod power;
mod usb;
use std::{fs, io::Write, path::Path, process::Command};
pub type Result<T = ()> = std::result::Result<T, String>;
pub fn io<T>(r: std::io::Result<T>) -> Result<T> {
    r.map_err(|e| e.to_string())
}
pub fn read(p: impl AsRef<Path>) -> Result<String> {
    io(fs::read_to_string(p))
}
pub fn write(p: impl AsRef<Path>, s: impl AsRef<[u8]>) -> Result {
    io(fs::write(p, s))
}
pub fn run(cmd: &str, args: &[&str]) -> Result {
    let status = io(Command::new(cmd).args(args).status())?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("{cmd} exited {status}"))
    }
}
pub fn root_pi() -> Result {
    let output = io(Command::new("id").arg("-u").output())?;
    if output.stdout != b"0\n" {
        return Err("run as root on the Pi".into());
    }
    if !read("/proc/device-tree/model")?.contains("Raspberry Pi 4") {
        return Err("target must be Raspberry Pi 4, not WSL".into());
    }
    Ok(())
}
pub fn entries(p: impl AsRef<Path>) -> Result<Vec<std::path::PathBuf>> {
    io(fs::read_dir(p))?
        .map(|e| io(e).map(|e| e.path()))
        .collect()
}
pub fn backup(path: &Path) -> Result {
    let name = path
        .file_name()
        .ok_or("invalid backup path")?
        .to_string_lossy();
    let original = path.with_file_name(format!("{name}.pi-cec-original"));
    let absent = path.with_file_name(format!("{name}.pi-cec-absent"));
    if !original.exists() && !absent.exists() {
        if path.exists() {
            io(fs::copy(path, original))?;
        } else {
            write(absent, [])?;
        }
    }
    Ok(())
}
pub fn saved_write(path: &Path, content: impl AsRef<[u8]>) -> Result {
    io(fs::create_dir_all(path.parent().ok_or("no parent")?))?;
    backup(path)?;
    write(path, content)
}
pub fn restore(path: &Path) -> Result {
    let name = path
        .file_name()
        .ok_or("invalid restore path")?
        .to_string_lossy();
    let original = path.with_file_name(format!("{name}.pi-cec-original"));
    let absent = path.with_file_name(format!("{name}.pi-cec-absent"));
    if original.exists() {
        io(fs::copy(original, path))?;
    } else if absent.exists() && path.exists() {
        io(fs::remove_file(path))?;
    }
    Ok(())
}
pub fn dispatch(args: &[String]) -> Option<Result> {
    let sub = args.first()?.as_str();
    let result =
        match sub {
            "inspect" => inspect(),
            "power-log" => power::record(&args[1..]),
            "power-report" => power::report(&args[1..]),
            "edid" => edid(&args[1..]),
            "prepare-boot" => boot::prepare(&args[1..]),
            "gadget" => root_pi()
                .and_then(|_| usb::gadget(args.get(1).map(String::as_str).unwrap_or("start"))),
            "trace" => root_pi()
                .and_then(|_| usb::trace(args.get(1).map(String::as_str).unwrap_or("start"))),
            "install" => install::install(),
            "uninstall" => install::uninstall(),
            "enable-live" => install::enable_live(),
            _ => return None,
        };
    Some(result)
}
pub fn setup_trace_for_daemon() -> Result {
    root_pi()?;
    usb::trace("start")
}
fn edid(args: &[String]) -> Result {
    if !(args.len() == 1 || args.len() == 3) {
        return Err("usage: pi-cec edid PI-ORIGINAL.bin [PC-ORIGINAL.bin OUTPUT.bin]".into());
    }
    let pi = io(fs::read(&args[0]))?;
    println!("Pi port PA: {:04x}", crate::edid::address(&pi)?);
    if args.len() == 3 {
        let pc = io(fs::read(&args[1]))?;
        println!("PC port PA: {:04x}", crate::edid::address(&pc)?);
        let data = crate::edid::proxy(&pi, &pc)?;
        let mut file = io(fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&args[2]))?;
        io(file.write_all(&data))?;
        println!(
            "Prepared {}; not installed. Apply only to Pi connector.",
            args[2]
        );
    }
    Ok(())
}
fn inspect() -> Result {
    println!("Read-only discovery. WSL output is not evidence about the Pi.");
    for path in [
        "/etc/os-release",
        "/proc/sys/kernel/osrelease",
        "/proc/device-tree/model",
        "/proc/sys/kernel/random/boot_id",
        "/proc/uptime",
        "/proc/cmdline",
        "/boot/firmware/config.txt",
        "/boot/firmware/usercfg.txt",
        "/boot/config.txt",
    ] {
        println!(
            "\n{path}:\n{}",
            read(path).unwrap_or_else(|e| format!("unavailable: {e}"))
        );
    }
    for dir in [
        "/sys/class/udc",
        "/sys/class/drm",
        "/sys/kernel/config/usb_gadget",
    ] {
        println!("\n{dir}:");
        match entries(dir) {
            Ok(paths) => {
                for path in paths {
                    println!("{}", path.display());
                    for field in ["state", "status", "uevent"] {
                        if let Ok(s) = read(path.join(field)) {
                            println!("{field}: {s}");
                        }
                    }
                }
            }
            Err(e) => println!("unavailable: {e}"),
        }
    }
    for (cmd, args) in [
        ("vcgencmd", vec!["get_throttled"]),
        ("cec-ctl", vec!["--list-devices"]),
        ("lsmod", vec![]),
        (
            "journalctl",
            vec!["-k", "-b", "--no-pager", "-p", "warning"],
        ),
    ] {
        if let Err(e) = run(cmd, &args) {
            println!("optional diagnostic unavailable: {e}");
        }
    }
    if let Ok(paths) = entries("/dev") {
        for p in paths {
            if p.file_name().unwrap().to_string_lossy().starts_with("cec") {
                let _ = run("cec-ctl", &["-d", &p.to_string_lossy()]);
            }
        }
    }
    let kernel = read("/proc/sys/kernel/osrelease")?;
    if let Ok(s) = read(format!("/boot/config-{}", kernel.trim())) {
        for l in s.lines().filter(|l| {
            [
                "CONFIG_USB_DWC2",
                "CONFIG_USB_CONFIGFS",
                "CONFIG_KPROBE_EVENTS",
                "CONFIG_CEC",
                "CONFIG_DRM_VC4",
                "CONFIG_DRM_LOAD_EDID_FIRMWARE",
            ]
            .iter()
            .any(|key| l.starts_with(key))
        }) {
            println!("{l}");
        }
    }
    Ok(())
}
