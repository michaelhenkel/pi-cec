use super::*;
use std::{
    io::{BufRead, BufReader},
    thread,
    time::{Duration, SystemTime, UNIX_EPOCH},
};
fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
fn hwmon_alarm() -> Option<bool> {
    for dir in entries("/sys/class/hwmon").ok()? {
        if read(dir.join("name")).is_ok_and(|name| name.trim() == "rpi_volt") {
            return match read(dir.join("in0_lcrit_alarm")).ok()?.trim() {
                "0" => Some(false),
                "1" => Some(true),
                _ => None,
            };
        }
    }
    None
}
fn sample() -> Result<String> {
    let boot = read("/proc/sys/kernel/random/boot_id")?;
    let uptime = read("/proc/uptime")?;
    let uptime = uptime.split_whitespace().next().ok_or("uptime missing")?;
    let bits = Command::new("vcgencmd")
        .arg("get_throttled")
        .output()
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .and_then(|s| {
            s.trim()
                .split_once('=')
                .and_then(|(_, v)| u32::from_str_radix(v.trim_start_matches("0x"), 16).ok())
        });
    let indicator = bits.map(|b| format!("{b:08x}")).unwrap_or_else(|| {
        hwmon_alarm()
            .map(|alarm| format!("hwmon:{}", u8::from(alarm)))
            .unwrap_or_else(|| "unavailable".into())
    });
    Ok(format!(
        "{} {} {} {}",
        now(),
        boot.trim(),
        uptime,
        indicator
    ))
}
pub fn record(args: &[String]) -> Result {
    if args.len() > 1 {
        return Err("usage: pi-cec power-log [LOG-PATH]".into());
    }
    let path = args
        .first()
        .map(String::as_str)
        .unwrap_or("/var/lib/pi-cec/power.log");
    let mut file = io(fs::OpenOptions::new().append(true).create(true).open(path))?;
    loop {
        let line = sample()?;
        println!("{line}");
        io(writeln!(file, "{line}"))?;
        io(file.sync_data())?;
        thread::sleep(Duration::from_secs(2));
    }
}
pub fn report(args: &[String]) -> Result {
    if args.len() != 1 {
        return Err("usage: pi-cec power-report LOG-PATH".into());
    }
    let file = io(fs::File::open(&args[0]))?;
    let mut summary = Summary::default();
    for line in BufReader::new(file).lines() {
        summary.add(&io(line)?)?;
    }
    println!("boot_ids={} undervoltage_samples={} incomplete_indicators={} gaps_over_10s={} observed_uptime_s={:.2}",summary.boots.len(),summary.under,summary.missing,summary.gaps,summary.duration);
    if summary.boots.len() > 1 || summary.under > 0 || summary.gaps > 0 {
        println!("FAILED continuity/voltage validation: inspect reboot cause and sample gaps. Do not enable AV control.");
    } else if summary.boots.is_empty() || summary.missing > 0 {
        println!("PENDING: incomplete voltage history; hwmon alone cannot prove absence of short undervoltage events. Review kernel logs or use firmware indicators.");
    } else {
        println!("No recorded reboot/undervoltage/gap. Hardware test remains PENDING until sleep, shutdown, idle and load phases are documented; this is not an automatic pass.");
    }
    Ok(())
}

#[derive(Default)]
struct Summary {
    boots: std::collections::BTreeSet<String>,
    last: Option<(String, f64)>,
    under: usize,
    missing: usize,
    gaps: usize,
    duration: f64,
}
impl Summary {
    fn add(&mut self, line: &str) -> Result {
        let f: Vec<_> = line.split_whitespace().collect();
        if f.len() != 4 {
            return Err("malformed power log".into());
        }
        let _ = f[0].parse::<u64>().map_err(|_| "invalid timestamp")?;
        let uptime = f[2].parse::<f64>().map_err(|_| "invalid uptime")?;
        if !uptime.is_finite() || uptime < 0.0 {
            return Err("invalid uptime".into());
        }
        self.boots.insert(f[1].into());
        if let Some((boot, previous)) = &self.last {
            if boot == f[1] {
                let elapsed = uptime - previous;
                if elapsed < 0.0 {
                    return Err("uptime moved backwards within same boot".into());
                }
                self.duration += elapsed;
                if elapsed > 10.0 {
                    self.gaps += 1;
                }
            }
        }
        self.last = Some((f[1].into(), uptime));
        match u32::from_str_radix(f[3], 16) {
            Ok(bits) => {
                if bits & 0x10001 != 0 {
                    self.under += 1;
                }
            }
            Err(_) => {
                self.missing += 1;
                if f[3] == "hwmon:1" {
                    self.under += 1;
                }
            }
        }
        Ok(())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ntp_clock_jump_is_not_a_power_gap() {
        let mut s = Summary::default();
        s.add("100 boot-a 20.0 00000000").unwrap();
        s.add("1000000 boot-a 22.0 00000000").unwrap();
        assert_eq!(s.gaps, 0);
        assert_eq!(s.duration, 2.0);
    }
    #[test]
    fn reboot_and_missing_samples_remain_visible() {
        let mut s = Summary::default();
        s.add("100 boot-a 20.0 00000000").unwrap();
        s.add("101 boot-a 40.0 00000000").unwrap();
        s.add("102 boot-b 1.0 00000000").unwrap();
        assert_eq!(s.gaps, 1);
        assert_eq!(s.boots.len(), 2);
    }
    #[test]
    fn hwmon_is_evidence_of_failure_but_not_complete_history() {
        let mut s = Summary::default();
        s.add("100 boot-a 20.0 hwmon:0").unwrap();
        s.add("102 boot-a 22.0 hwmon:1").unwrap();
        assert_eq!(s.missing, 2);
        assert_eq!(s.under, 1);
    }
}
