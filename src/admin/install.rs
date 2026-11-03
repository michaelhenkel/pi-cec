use super::*;
use std::os::unix::fs::PermissionsExt;
const BIN: &str = "/usr/local/bin/pi-cec";
const CONF: &str = "/etc/pi-cec/pi-cec.conf";
const MODE: &str = "/etc/pi-cec/mode";
const UNITS: [(&str, &[u8]); 3] = [
    (
        "/etc/systemd/system/pi-cec.service",
        include_bytes!("../../systemd/pi-cec.service"),
    ),
    (
        "/etc/systemd/system/pi-cec-gadget.service",
        include_bytes!("../../systemd/pi-cec-gadget.service"),
    ),
    (
        "/etc/systemd/system/pi-cec-power.service",
        include_bytes!("../../systemd/pi-cec-power.service"),
    ),
];
fn put(path: &str, bytes: &[u8], mode: u32) -> Result {
    let target = Path::new(path);
    io(fs::create_dir_all(
        target.parent().ok_or("invalid installation path")?,
    ))?;
    backup(target)?;
    let temporary = target.with_extension("pi-cec-new");
    write(&temporary, bytes)?;
    io(fs::set_permissions(
        &temporary,
        fs::Permissions::from_mode(mode),
    ))?;
    io(fs::rename(temporary, target))
}
pub fn install() -> Result {
    root_pi()?;
    let binary = io(fs::read(io(std::env::current_exe())?))?;
    put(BIN, &binary, 0o755)?;
    for (path, data) in UNITS {
        put(path, data, 0o644)?;
    }
    if !Path::new(CONF).exists() {
        put(CONF, include_bytes!("../../config/pi-cec.conf"), 0o644)?;
    }
    if !Path::new(MODE).exists() {
        put(MODE, b"DAEMON_ARGS=--dry-run\n", 0o644)?;
    }
    io(fs::create_dir_all("/var/lib/pi-cec"))?;
    run("systemctl", &["daemon-reload"])?;
    run("systemctl", &["enable", "--now", "pi-cec-power.service"])?;
    println!("Installed. Only power logger enabled. Complete commissioning before USB/AV startup.");
    Ok(())
}
pub fn enable_live() -> Result {
    root_pi()?;
    crate::config::Config::load(CONF)?.live_check()?;
    let old = read(MODE)?;
    write("/var/lib/pi-cec/mode-before-live", old)?;
    saved_write(Path::new(MODE), "DAEMON_ARGS=\n")?;
    run(
        "systemctl",
        &["enable", "pi-cec-gadget.service", "pi-cec.service"],
    )?;
    run("systemctl", &["restart", "pi-cec.service"])
}
pub fn uninstall() -> Result {
    root_pi()?;
    let _ = run(
        "systemctl",
        &[
            "disable",
            "--now",
            "pi-cec.service",
            "pi-cec-gadget.service",
            "pi-cec-power.service",
        ],
    );
    super::usb::trace("stop")?;
    for path in UNITS.iter().map(|(p, _)| *p).chain([CONF, MODE, BIN]) {
        let original = format!("{path}.pi-cec-original");
        let absent = format!("{path}.pi-cec-absent");
        if !Path::new(&original).exists() && !Path::new(&absent).exists() {
            continue;
        }
        // Preserve the latest version before restoring the original baseline.
        if Path::new(path).exists() {
            io(fs::copy(path, format!("{path}.pi-cec-uninstalled")))?;
        }
        restore(Path::new(path))?;
        // Retire the baseline so a later fresh install preserves changes made
        // by the administrator after this uninstall.
        for marker in [&original, &absent] {
            if Path::new(marker).exists() {
                io(fs::rename(marker, format!("{marker}-uninstalled")))?;
            }
        }
    }
    run("systemctl", &["daemon-reload"])?;
    println!("Restored/removed managed files; logs and backup files retained. Run the preserved binary's prepare-boot --remove, then reboot.");
    Ok(())
}
