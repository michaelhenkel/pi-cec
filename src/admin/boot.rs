use super::*;
use std::os::unix::fs::PermissionsExt;
const BEGIN: &str = "# BEGIN PI-CEC MANAGED";
const END: &str = "# END PI-CEC MANAGED";
const QUIET_BEGIN: &str = "# BEGIN PI-CEC BOOT CEC";
const QUIET_END: &str = "# END PI-CEC BOOT CEC";
const POWER_BEGIN: &str = "# BEGIN PI-CEC CPU LIMIT";
const POWER_END: &str = "# END PI-CEC CPU LIMIT";
const FW: &str = "/lib/firmware/pi-cec/proxy.bin";
const HOOK: &str = "/etc/initramfs-tools/hooks/pi-cec-edid";
const DRACUT: &str = "/etc/dracut.conf.d/90-pi-cec-edid.conf";
const DRACUT_TEXT: &str = "install_items+=\" /lib/firmware/pi-cec/proxy.bin \"\n";
// initramfs-tools defines its hook protocol in shell. This is the only shell
// glue retained; all policy, file preparation and lifecycle code is Rust.
const HOOK_TEXT:&str="#!/bin/sh\nset -e\ncase \"$1\" in prereqs) exit 0;; esac\nmkdir -p \"${DESTDIR}/lib/firmware/pi-cec\"\ncp /lib/firmware/pi-cec/proxy.bin \"${DESTDIR}/lib/firmware/pi-cec/proxy.bin\"\n";
#[derive(Debug, PartialEq)]
enum Initramfs {
    Dracut,
    Tools,
}
fn backend_from_owner(owner: &str) -> Result<Initramfs> {
    match owner.trim() {
        "dracut: /usr/sbin/update-initramfs" => Ok(Initramfs::Dracut),
        "initramfs-tools: /usr/sbin/update-initramfs" => Ok(Initramfs::Tools),
        _ => {
            Err("unsupported update-initramfs provider; inspect installed boot tools first".into())
        }
    }
}
fn initramfs_backend() -> Result<Initramfs> {
    let output = io(Command::new("dpkg-query")
        .args(["-S", "/usr/sbin/update-initramfs"])
        .output())?;
    if !output.status.success() {
        return Err("cannot identify installed update-initramfs provider".into());
    }
    backend_from_owner(&String::from_utf8_lossy(&output.stdout))
}
fn remove_block(text: &str) -> Result<String> {
    remove_named_block(text, BEGIN, END)
}
fn remove_named_block(text: &str, begin: &str, end: &str) -> Result<String> {
    if text.matches(begin).count() > 1 || text.matches(end).count() > 1 {
        return Err("duplicate managed boot block; inspect manually".into());
    }
    match (text.find(begin), text.find(end)) {
        (None, None) => Ok(text.into()),
        (Some(a), Some(b)) if b > a => Ok(format!("{}{}", &text[..a], &text[b + end.len()..])),
        _ => Err("malformed managed boot block; inspect manually".into()),
    }
}
fn quiet_config(text: &str) -> Result<String> {
    let clean = remove_named_block(text, QUIET_BEGIN, QUIET_END)?;
    Ok(format!("{}\n\n{QUIET_BEGIN}\n[all]\nhdmi_ignore_cec_init=1\nhdmi_ignore_cec_init:1=1\n{QUIET_END}\n", clean.trim_end()))
}
fn low_power_config(text: &str) -> Result<String> {
    let clean = remove_named_block(text, POWER_BEGIN, POWER_END)?;
    Ok(format!("{}\n\n{POWER_BEGIN}\n[all]\narm_boost=0\ninitial_turbo=0\narm_freq=600\narm_freq_min=600\n{POWER_END}\n", clean.trim_end()))
}
fn proxy_command(command: &str, connector: &str) -> Result<String> {
    let mut tokens: Vec<String> = command.split_whitespace().map(str::to_owned).collect();
    for (prefix, wanted) in [
        ("drm.edid_firmware=".into(), format!("drm.edid_firmware={connector}:pi-cec/proxy.bin")),
        (format!("video={connector}:"), format!("video={connector}:1280x720@60D")),
    ] {
        let existing: Vec<_> = tokens.iter().filter(|t| t.starts_with(&prefix)).collect();
        if existing.is_empty() { tokens.push(wanted); }
        else if existing != vec![&wanted] { return Err(format!("conflicting {prefix} boot option; reconcile before changing")); }
    }
    Ok(format!("{}\n", tokens.join(" ")))
}
fn remove_proxy_command(command: &str, baseline: &str) -> String {
    let video = command.split_whitespace().find_map(|t| {
        t.strip_prefix("drm.edid_firmware=").and_then(|value| value.strip_suffix(":pi-cec/proxy.bin"))
            .map(|connector| format!("video={connector}:1280x720@60D"))
    });
    let tokens: Vec<_> = command.split_whitespace().filter(|t| {
        !(t.starts_with("drm.edid_firmware=") && t.contains("pi-cec/proxy.bin"))
            && !(video.as_deref() == Some(*t) && !baseline.split_whitespace().any(|b| b == *t))
    }).collect();
    format!("{}\n", tokens.join(" "))
}
fn cmdline_relative(text: &str) -> Result<&'static str> {
    let prefixes: Vec<_> = text
        .lines()
        .filter_map(|line| {
            line.split('#')
                .next()
                .unwrap_or("")
                .trim()
                .strip_prefix("os_prefix=")
        })
        .filter(|prefix| !prefix.is_empty())
        .collect();
    if prefixes.is_empty() {
        return Ok("cmdline.txt");
    }
    if prefixes.contains(&"current/") && prefixes.iter().all(|p| matches!(*p, "current/" | "new/"))
    {
        return Ok("current/cmdline.txt");
    }
    Err("unsupported os_prefix boot layout; no files changed".into())
}
pub fn prepare(args: &[String]) -> Result {
    root_pi()?;
    let (mut usb, mut remove, mut quiet, mut proxy, mut connector) =
        (false, false, false, None, None);
    let mut low_power = false;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--usb" => usb = true,
            "--quiet-cec" => quiet = true,
            "--low-power" => low_power = true,
            "--remove" => remove = true,
            "--proxy-edid" => {
                i += 1;
                proxy = Some(args.get(i).ok_or("missing EDID path")?.clone());
            }
            "--connector" => {
                i += 1;
                connector = Some(args.get(i).ok_or("missing connector")?.clone());
            }
            _ => {
                return Err(
                    "usage: prepare-boot --quiet-cec | --low-power | --usb | --proxy-edid FILE --connector HDMI-A-1 | --remove"
                        .into(),
                )
            }
        }
        i += 1;
    }
    if remove && (usb || proxy.is_some()) {
        return Err("--remove cannot be combined with other operations".into());
    }
    if (quiet || low_power) && (usb || proxy.is_some() || remove || connector.is_some() || (quiet && low_power)) {
        return Err("--quiet-cec and --low-power must each be used alone".into());
    }
    let boot = if Path::new("/boot/firmware/config.txt").exists() {
        Path::new("/boot/firmware")
    } else {
        Path::new("/boot")
    };
    let config = boot.join("config.txt");
    let text = read(&config)?;
    let relative = cmdline_relative(&text)?;
    if relative.starts_with("current/") && boot.join("new").exists() {
        return Err("Ubuntu has pending new boot assets; complete and validate that boot before changing configuration".into());
    }
    if quiet {
        saved_write(&config, quiet_config(&text)?)?;
        println!("Prepared firmware CEC startup suppression with config.txt.pi-cec-original backup. Reboot and observe receiver input to verify; this legacy option is not proof of quiet boot on this firmware. Native Linux CEC remains enabled.");
        return Ok(());
    }
    if low_power {
        saved_write(&config, low_power_config(&text)?)?;
        println!("Prepared 600 MHz CPU limit and disabled initial turbo; backup retained. Reboot, check effective clocks and repeat voltage/load tests. This does not certify the USB supply.");
        return Ok(());
    }
    let cmdline = boot.join(relative);
    let command = read(&cmdline)?;
    if command.trim().contains('\n') {
        return Err("unsupported multiline cmdline.txt".into());
    }
    let clean = remove_block(&text)?;
    // Ubuntu 26.04's update-initramfs comes from dracut. Merely finding this
    // command does not establish that initramfs-tools hooks will be executed.
    let had_proxy = command.contains("pi-cec/proxy.bin") || Path::new(FW).exists();
    let backend = if (remove && had_proxy) || proxy.is_some() {
        Some(initramfs_backend()?)
    } else {
        None
    };
    if remove {
        let clean = remove_named_block(&clean, QUIET_BEGIN, QUIET_END)?;
        saved_write(&config, remove_named_block(&clean, POWER_BEGIN, POWER_END)?)?;
        let baseline = read(cmdline.with_file_name("cmdline.txt.pi-cec-original")).unwrap_or_default();
        saved_write(&cmdline, remove_proxy_command(&command, &baseline))?;
        restore(Path::new(HOOK))?;
        restore(Path::new(DRACUT))?;
        restore(Path::new(FW))?;
        if had_proxy {
            run("update-initramfs", &["-u"])?;
        }
        println!("Removed managed boot options, preserved other changes. Reboot required.");
        return Ok(());
    }
    if !usb && proxy.is_none() {
        return Err("choose --usb or --proxy-edid".into());
    }
    // Ubuntu already provides dwc2 in OTG mode. A live UDC proves device mode
    // is available; don't duplicate overlays or mistake the [cm4] host setting
    // elsewhere in config.txt for the Pi 4's active configuration.
    if usb
        && entries("/sys/class/udc")
            .map(|v| v.len() == 1)
            .unwrap_or(false)
    {
        println!(
            "USB peripheral controller already available; preserving existing boot configuration"
        );
        usb = false;
    }
    if usb
        && clean
            .lines()
            .map(str::trim)
            .any(|l| l.starts_with("dtoverlay=dwc2") || l == "otg_mode=1")
    {
        return Err(
            "existing USB boot settings require reconciliation with discovery first".into(),
        );
    }
    if usb
        && command
            .split(|ch: char| ch.is_whitespace() || ch == ',')
            .any(|s| ["g_serial", "g_ether", "g_multi", "g_hid", "g_mass_storage"].contains(&s))
    {
        return Err("legacy gadget in cmdline; reconcile first".into());
    }
    let mut new_command = command.clone();
    let mut data = None;
    if let Some(file) = proxy {
        let name = connector.ok_or("--connector required; discover correct DRM connector")?;
        if !["HDMI-A-1", "HDMI-A-2"].contains(&name.as_str()) {
            return Err("invalid connector".into());
        }
        if !text
            .lines()
            .map(str::trim)
            .any(|l| l.starts_with("dtoverlay=vc4-kms-v3d"))
        {
            return Err(
                "full KMS overlay not found; inspect distro includes before changing boot".into(),
            );
        }
        let bytes = io(fs::read(file))?;
        println!(
            "Pi connector {name} will use proxy PA {:04x}",
            crate::edid::address(&bytes)?
        );
        new_command = proxy_command(&command, &name)?;
        data = Some(bytes);
    }
    if usb {
        saved_write(
            &config,
            format!(
                "{}\n{BEGIN}\n[all]\ndtoverlay=dwc2,dr_mode=peripheral\n{END}\n",
                clean.trim_end()
            ),
        )?;
    }
    if let Some(bytes) = data {
        saved_write(Path::new(FW), bytes)?;
        match backend.ok_or("missing initramfs backend")? {
            Initramfs::Dracut => {
                saved_write(Path::new(DRACUT), DRACUT_TEXT)?;
                restore(Path::new(HOOK))?;
            }
            Initramfs::Tools => {
                saved_write(Path::new(HOOK), HOOK_TEXT)?;
                io(fs::set_permissions(HOOK, fs::Permissions::from_mode(0o755)))?;
                restore(Path::new(DRACUT))?;
            }
        }
        saved_write(&cmdline, new_command)?;
        run("update-initramfs", &["-u"])?;
    }
    println!("Boot settings prepared with .pi-cec-original backups. Reboot required.");
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn removes_only_owned_block() {
        let t = format!("other=1\n{BEGIN}\n[all]\nx=1\n{END}\nother=2\n");
        let out = remove_block(&t).unwrap();
        assert!(out.contains("other=1"));
        assert!(out.contains("other=2"));
        assert!(!out.contains("x=1"));
    }
    #[test]
    fn broken_block_rejected() {
        assert!(remove_block(BEGIN).is_err());
    }
    #[test]
    fn quiet_boot_is_idempotent_and_preserves_usb_configuration() {
        let original = format!(
            "[all]\nos_prefix=current/\n{BEGIN}\n[all]\ndtoverlay=dwc2,dr_mode=peripheral\n{END}\n"
        );
        let quiet = quiet_config(&original).unwrap();
        assert_eq!(quiet_config(&quiet).unwrap(), quiet);
        assert!(quiet.starts_with(original.trim_end()));
        assert!(!quiet
            .lines()
            .any(|line| line.starts_with("hdmi_ignore_cec=")));
        assert_eq!(
            remove_named_block(&quiet, QUIET_BEGIN, QUIET_END)
                .unwrap()
                .trim_end(),
            original.trim_end()
        );
        assert!(quiet_config(QUIET_BEGIN).is_err());
    }
    #[test]
    fn cpu_limit_preserves_quiet_boot_and_is_removable() {
        let quiet = quiet_config("[all]\narm_boost=1\n").unwrap();
        let limited = low_power_config(&quiet).unwrap();
        assert_eq!(low_power_config(&limited).unwrap(), limited);
        assert!(limited.starts_with(quiet.trim_end()));
        assert_eq!(remove_named_block(&limited, POWER_BEGIN, POWER_END).unwrap().trim_end(), quiet.trim_end());
    }
    #[test]
    fn proxy_boot_preserves_other_connector_and_removes_only_added_options() {
        let baseline = "root=LABEL=writable video=HDMI-A-2:d\n";
        let proxy = proxy_command(baseline, "HDMI-A-1").unwrap();
        assert_eq!(proxy_command(&proxy, "HDMI-A-1").unwrap(), proxy);
        assert_eq!(remove_proxy_command(&proxy, baseline), baseline);
        let existing = "root=x video=HDMI-A-1:1280x720@60D\n";
        assert_eq!(remove_proxy_command(&proxy_command(existing, "HDMI-A-1").unwrap(), existing), existing);
        assert!(proxy_command("video=HDMI-A-1:d", "HDMI-A-1").is_err());
        assert!(proxy_command("drm.edid_firmware=other.bin", "HDMI-A-1").is_err());
    }
    #[test]
    fn ubuntu_prefixed_boot_uses_current_cmdline() {
        assert_eq!(
            cmdline_relative("[all]\nos_prefix=current/\n[tryboot]\nos_prefix=new/\n").unwrap(),
            "current/cmdline.txt"
        );
        assert_eq!(cmdline_relative("[all]\n").unwrap(), "cmdline.txt");
        assert!(cmdline_relative("os_prefix=custom/\n").is_err());
    }
    #[test]
    fn initramfs_provider_must_be_identified() {
        assert_eq!(
            backend_from_owner("dracut: /usr/sbin/update-initramfs\n").unwrap(),
            Initramfs::Dracut
        );
        assert_eq!(
            backend_from_owner("initramfs-tools: /usr/sbin/update-initramfs\n").unwrap(),
            Initramfs::Tools
        );
        assert!(backend_from_owner("unknown: /usr/sbin/update-initramfs\n").is_err());
        assert!(backend_from_owner("").is_err());
    }
}
