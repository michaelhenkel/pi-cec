use pi_cec::Usb;
use std::{
    fs,
    io::{BufRead, BufReader},
    sync::mpsc::{self, Receiver},
    thread,
};

pub struct UsbReader {
    path: String,
    trace: Option<Receiver<Result<String, String>>>,
    latched: Usb,
    last_sysfs: String,
    validated: bool,
}
impl UsbReader {
    pub fn open(udc: &str, backend: &str, pipe: &str, validated: bool) -> Result<Self, String> {
        let entries: Vec<_> = fs::read_dir("/sys/class/udc")
            .map_err(|e| e.to_string())?
            .filter_map(Result::ok)
            .collect();
        // Kprobe events have a single-controller scope. Refuse ambiguous hosts.
        if entries.len() != 1 {
            return Err(format!("expected exactly one UDC; found {}", entries.len()));
        }
        let name = entries[0].file_name().to_string_lossy().into_owned();
        if !udc.is_empty() && udc != name {
            return Err(format!("configured UDC {udc} missing"));
        }
        let path = format!("/sys/class/udc/{name}/state");
        let initial = fs::read_to_string(&path).map_err(|e| e.to_string())?;
        let trace = if backend == "trace" {
            let file =
                fs::File::open(pipe).map_err(|e| format!("trace bridge unavailable: {e}"))?;
            let (tx, rx) = mpsc::sync_channel(1024);
            thread::spawn(move || {
                for line in BufReader::new(file).lines() {
                    match line {
                        Ok(s) => {
                            if tx.try_send(Ok(s)).is_err() {
                                let _ = tx
                                    .send(Err("USB trace consumer overflow; state unknown".into()));
                                return;
                            }
                        }
                        Err(e) => {
                            let _ = tx.send(Err(e.to_string()));
                            return;
                        }
                    }
                }
                let _ = tx.send(Err("USB trace ended".into()));
            });
            Some(rx)
        } else {
            None
        };
        Ok(Self {
            path,
            trace,
            latched: Usb::Unknown,
            last_sysfs: initial.trim().into(),
            validated,
        })
    }
    pub fn sample(&mut self) -> Result<Usb, String> {
        let state = fs::read_to_string(&self.path).map_err(|e| e.to_string())?;
        let state = state.trim();
        if let Some(rx) = &self.trace {
            let mut saw_event = false;
            loop {
                match rx.try_recv() {
                    Ok(Ok(line)) => {
                        // Journald timestamps this raw kernel event alongside
                        // the sysfs snapshot, so commissioning can establish
                        // whether sysfs actually follows suspend and resume.
                        eprintln!("USB trace: {line}; sysfs={state}");
                        if line.contains("LOST") {
                            self.latched = Usb::Unknown;
                            return Err(line);
                        }
                        if line.contains("pc_suspend:")
                            || line.contains("pc_disconnect:")
                            || line.contains("pc_reset:")
                        {
                            self.latched = Usb::Inactive;
                            saw_event = true;
                        } else if line.contains("pc_resume:") && state == "configured" {
                            self.latched = Usb::Active;
                            saw_event = true;
                        } else if line.contains("usb_gadget_set_state:") {
                            saw_event = true;
                            if line.contains("state 7 ") {
                                self.latched = Usb::Active;
                            } else {
                                self.latched = Usb::Inactive;
                            }
                        }
                    }
                    Ok(Err(e)) => {
                        self.latched = Usb::Unknown;
                        return Err(e);
                    }
                    Err(mpsc::TryRecvError::Empty) => break,
                    Err(mpsc::TryRecvError::Disconnected) => {
                        return Err("trace reader disconnected".into())
                    }
                }
            }
            // Polling can detect removal, but cannot overrule a suspend latch
            // with a stale 'configured' value. Startup configured stays UNKNOWN.
            self.latched =
                reconcile_trace_snapshot(state, &self.last_sysfs, self.latched, saw_event);
        } else {
            self.latched = match state {
                "configured" => Usb::Active,
                "not attached" | "powered" | "default" | "addressed" | "suspended" => Usb::Inactive,
                _ => Usb::Unknown,
            };
            if !self.validated {
                self.latched = Usb::Unknown;
            }
        }
        self.last_sysfs = state.into();
        Ok(self.latched)
    }
}

fn reconcile_trace_snapshot(state: &str, previous: &str, latched: Usb, saw_event: bool) -> Usb {
    if state != "configured" {
        Usb::Inactive
    } else if previous != "configured" && !saw_event {
        Usb::Active
    } else {
        // Configuration followed by suspension can arrive in one poll. A
        // stale configured snapshot must not erase the latest suspend event.
        latched
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn suspend_after_configuration_wins_over_snapshot_edge() {
        assert_eq!(
            reconcile_trace_snapshot("configured", "addressed", Usb::Inactive, true),
            Usb::Inactive
        );
        assert_eq!(
            reconcile_trace_snapshot("configured", "configured", Usb::Inactive, false),
            Usb::Inactive
        );
        assert_eq!(
            reconcile_trace_snapshot("configured", "configured", Usb::Active, true),
            Usb::Active
        );
    }

    #[test]
    fn startup_snapshot_is_unknown_until_a_real_transition() {
        assert_eq!(
            reconcile_trace_snapshot("configured", "configured", Usb::Unknown, false),
            Usb::Unknown
        );
        assert_eq!(
            reconcile_trace_snapshot("configured", "addressed", Usb::Unknown, false),
            Usb::Active
        );
        assert_eq!(
            reconcile_trace_snapshot("not attached", "configured", Usb::Active, true),
            Usb::Inactive
        );
    }
}
