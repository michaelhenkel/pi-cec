use super::*;
use std::{os::unix::fs::symlink, path::PathBuf};
const TRACE: &str = "/sys/kernel/tracing";
fn controller() -> Result<PathBuf> {
    let list = entries("/sys/class/udc")?;
    if list.len() != 1 {
        return Err(format!("expected one UDC, found {}", list.len()));
    }
    Ok(list[0].clone())
}
pub fn gadget(action: &str) -> Result {
    let g = Path::new("/sys/kernel/config/usb_gadget/pi-cec");
    if action == "stop" {
        if !g.exists() {
            return Ok(());
        }
        write(g.join("UDC"), "\n")?;
        let link = g.join("configs/c.1/acm.usb0");
        if link.exists() {
            io(fs::remove_file(link))?;
        }
        for path in [
            "functions/acm.usb0",
            "configs/c.1/strings/0x409",
            "configs/c.1",
            "strings/0x409",
            "",
        ] {
            io(fs::remove_dir(g.join(path)))?;
        }
        return Ok(());
    }
    if action != "start" {
        return Err("gadget expects start or stop".into());
    }
    run("modprobe", &["dwc2"])?;
    run("modprobe", &["libcomposite"])?;
    // Capture the first enumeration before binding. The daemon can consume
    // these events after systemd starts it, including a following suspend.
    trace("start")?;
    if !Path::new("/sys/kernel/config/usb_gadget").exists() {
        run("mount", &["-t", "configfs", "none", "/sys/kernel/config"])?;
    }
    if read(g.join("UDC"))
        .map(|s| !s.trim().is_empty())
        .unwrap_or(false)
    {
        return Ok(());
    }
    let udc = controller()?;
    for path in entries("/sys/kernel/config/usb_gadget")? {
        if path != g
            && read(path.join("UDC"))
                .map(|s| !s.trim().is_empty())
                .unwrap_or(false)
        {
            return Err(format!("another gadget is bound: {}", path.display()));
        }
    }
    io(fs::create_dir_all(g))?;
    for (field, value) in [
        ("idVendor", "0x1d6b"),
        ("idProduct", "0x0104"),
        ("bcdUSB", "0x0200"),
        ("bcdDevice", "0x0100"),
        ("bDeviceClass", "0x02"),
        ("bDeviceSubClass", "0x02"),
        ("bDeviceProtocol", "0x00"),
    ] {
        write(g.join(field), value)?;
    }
    io(fs::create_dir_all(g.join("strings/0x409")))?;
    let serial = read("/proc/device-tree/serial-number")?;
    write(
        g.join("strings/0x409/serialnumber"),
        format!("pi-cec-{}", serial.trim_matches('\0')),
    )?;
    write(
        g.join("strings/0x409/manufacturer"),
        "Personal AV Appliance",
    )?;
    write(g.join("strings/0x409/product"), "Pi CEC bus detector")?;
    io(fs::create_dir_all(g.join("configs/c.1/strings/0x409")))?;
    write(
        g.join("configs/c.1/strings/0x409/configuration"),
        "CEC host state",
    )?;
    let self_powered = std::env::var("GADGET_SELF_POWERED").unwrap_or_else(|_| "0".into());
    if !["0", "1"].contains(&self_powered.as_str()) {
        return Err("GADGET_SELF_POWERED must be 0 or 1".into());
    }
    write(
        g.join("configs/c.1/bmAttributes"),
        if self_powered == "1" { "0xc0" } else { "0x80" },
    )?;
    write(
        g.join("configs/c.1/MaxPower"),
        if self_powered == "1" { "0" } else { "500" },
    )?;
    // Descriptor current is not a guarantee of enough current for a Pi 4.
    io(fs::create_dir_all(g.join("functions/acm.usb0")))?;
    let link = g.join("configs/c.1/acm.usb0");
    if !link.exists() {
        io(symlink(g.join("functions/acm.usb0"), link))?;
    }
    write(
        g.join("UDC"),
        udc.file_name()
            .ok_or("invalid UDC name")?
            .as_encoded_bytes(),
    )?;
    Ok(())
}
pub fn trace(action: &str) -> Result {
    let root = Path::new(TRACE);
    let instance = root.join("instances/pi-cec");
    if !root.join("events").exists() {
        run("mount", &["-t", "tracefs", "tracefs", TRACE])?;
    }
    let probes = [
        ("pc_suspend", "composite_suspend"),
        ("pc_resume", "composite_resume"),
        ("pc_disconnect", "composite_disconnect"),
        ("pc_reset", "composite_reset"),
    ];
    fn append(path: &Path, s: &str) -> Result {
        let mut file = io(fs::OpenOptions::new().append(true).open(path))?;
        io(writeln!(file, "{s}"))
    }
    if action == "stop" {
        if instance.exists() {
            write(instance.join("tracing_on"), "0")?;
            write(instance.join("events/enable"), "0")?;
            io(fs::remove_dir(&instance))?;
        }
        for (event, _) in probes {
            if root.join(format!("events/pi_cec/{event}")).exists() {
                append(&root.join("kprobe_events"), &format!("-:pi_cec/{event}"))?;
            }
        }
        return Ok(());
    }
    if action != "start" {
        return Err("trace expects start or stop".into());
    }
    run("modprobe", &["libcomposite"])?;
    controller()?;
    if !root
        .join("events/gadget/usb_gadget_set_state/format")
        .exists()
    {
        return Err("required gadget state tracepoint missing".into());
    }
    let symbols = read("/proc/kallsyms")?;
    for (_, symbol) in probes {
        if !symbols
            .lines()
            .any(|l| l.split_whitespace().nth(2) == Some(symbol))
        {
            return Err(format!(
                "missing probe {symbol}; validate another bridge on this kernel"
            ));
        }
    }
    // Preserve unread boot/reconnection events when the daemon starts or
    // restarts. Resetting an already armed bridge loses initial enumeration.
    if read(instance.join("tracing_on")).is_ok_and(|v| v.trim() == "1")
        && read(instance.join("events/gadget/usb_gadget_set_state/enable"))
            .is_ok_and(|v| v.trim() == "1")
        && probes.iter().all(|(event, _)| {
            read(instance.join(format!("events/pi_cec/{event}/enable")))
                .is_ok_and(|v| v.trim() == "1")
        })
    {
        return Ok(());
    }
    io(fs::create_dir_all(&instance))?;
    write(instance.join("tracing_on"), "0")?;
    write(instance.join("events/enable"), "0")?;
    write(instance.join("trace"), "")?;
    write(instance.join("buffer_size_kb"), "128")?;
    write(instance.join("trace_clock"), "mono")?;
    for (event, symbol) in probes {
        if !root.join(format!("events/pi_cec/{event}")).exists() {
            append(
                &root.join("kprobe_events"),
                &format!("p:pi_cec/{event} {symbol}"),
            )?;
        }
        write(instance.join(format!("events/pi_cec/{event}/enable")), "1")?;
    }
    write(
        instance.join("events/gadget/usb_gadget_set_state/enable"),
        "1",
    )?;
    write(instance.join("tracing_on"), "1")?;
    Ok(())
}
