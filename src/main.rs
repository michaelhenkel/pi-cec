mod admin;
mod config;
mod edid;
mod usb;
use config::Config;
use pi_cec::{Action, Machine, Owner, State, Usb};
use std::{
    ffi::CString,
    os::raw::{c_char, c_int},
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

extern "C" {
    fn pc_cec_open(path: *const c_char, pa: u16, la: *mut u8, playback: c_int) -> c_int;
    fn pc_cec_monitor(path: *const c_char) -> c_int;
    fn pc_cec_monitor_close(fd: c_int);
    fn pc_cec_close(fd: c_int);
    fn pc_cec_tx(fd: c_int, data: *const u8, len: u32, pa: u16) -> c_int;
    fn pc_cec_next(
        fd: c_int,
        data: *mut u8,
        len: *mut u32,
        status: *mut u32,
        seq: *mut u32,
        pa: u16,
        la: u8,
    ) -> c_int;
}
fn log(s: impl std::fmt::Display) {
    let t = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    eprintln!("{}.{:03} {s}", t.as_secs(), t.subsec_millis());
}
struct Cec {
    fd: i32,
    la: u8,
    dry: bool,
    pa: u16,
}
enum CecEvent {
    Received(Vec<u8>),
    Transmitted(Vec<u8>, u32),
    Consumed,
}
impl Drop for Cec {
    fn drop(&mut self) {
        unsafe {
            if self.dry {
                pc_cec_monitor_close(self.fd)
            } else {
                pc_cec_close(self.fd)
            }
        }
    }
}
impl Cec {
    fn open(c: &Config, dry: bool) -> Result<Self, String> {
        let path = CString::new(c.adapter.as_str()).map_err(|e| e.to_string())?;
        let mut la = 15;
        let fd = unsafe {
            if dry {
                pc_cec_monitor(path.as_ptr())
            } else {
                pc_cec_open(path.as_ptr(), c.pc_pa, &mut la, c.playback as i32)
            }
        };
        if fd < 0 {
            return Err(format!("CEC open errno={}: missing/ambiguous adapter, unavailable playback LA, incorrect proxy PA or competing owner",-fd));
        }
        log(format!("CEC connected logical={la} dry={dry}"));
        Ok(Self {
            fd,
            la,
            dry,
            pa: c.pc_pa,
        })
    }
    fn tx(&self, dst: u8, payload: &[u8]) -> bool {
        let mut frame = vec![(self.la << 4) | dst];
        frame.extend_from_slice(payload);
        if self.dry {
            log(format!("DRY CEC TX {frame:02x?}"));
            return true;
        }
        let seq = unsafe { pc_cec_tx(self.fd, frame.as_ptr(), frame.len() as u32, self.pa) };
        log(format!("CEC TX queued seq={seq} frame={frame:02x?}"));
        seq >= 0
    }
    fn next(&self) -> Result<Option<CecEvent>, String> {
        let mut data = [0u8; 16];
        let (mut len, mut status, mut seq) = (0, 0, 0);
        let kind = unsafe {
            pc_cec_next(
                self.fd,
                data.as_mut_ptr(),
                &mut len,
                &mut status,
                &mut seq,
                self.pa,
                self.la,
            )
        };
        match kind {
            0 => Ok(None),
            1 => {
                let f = data[..len as usize].to_vec();
                log(format!("CEC RX {f:02x?} status={status:02x}"));
                Ok(Some(CecEvent::Received(f)))
            }
            2 => {
                log(format!(
                    "CEC TX complete seq={seq} status={status:02x} (not routing proof)"
                ));
                Ok(Some(CecEvent::Transmitted(data[..len as usize].to_vec(), status)))
            }
            3 => Err("CEC state change or queue loss; invalidate ownership".into()),
            4 => Ok(Some(CecEvent::Consumed)),
            _ => Err(format!("CEC receive errno={}", -kind)),
        }
    }
    fn announce(&self, pa: u16) {
        self.tx(15, &[0x82, (pa >> 8) as u8, pa as u8]);
    }
}
#[derive(Default, Debug)]
struct Observed {
    tv_power: Option<u8>,
    avr_power: Option<u8>,
    route_pc: bool,
}
struct Wake {
    generation: u64,
    attempt: u32,
    stage: u8,
    at: u64,
}

fn route_owner(pa: u16, pc: u16) -> Owner {
    if pa == pc {
        return Owner::Pc;
    }
    // A receiver branch does not identify the input selected within it.
    if pa != 0 && pi_cec::valid_pa(pa) {
        for shift in [8, 4, 0] {
            if (pa >> shift) & 15 == 0 {
                let mask = 0xffffu16 << (shift + 4);
                if pc & mask == pa & mask {
                    return Owner::Unknown;
                }
                break;
            }
        }
    }
    Owner::Other
}

fn receive(f: &[u8], cec: &Cec, c: &Config, m: &mut Machine, o: &mut Observed, now: u64) {
    if f.len() < 2 {
        return;
    }
    let src = f[0] >> 4;
    let dst = f[0] & 15;
    let op = f[1];
    if src == cec.la {
        return;
    } // never count our own announcements as evidence
    if op == 0x36 && (src == c.tv || src == c.avr) && (dst == 15 || dst == cec.la) {
        m.yield_session();
        o.route_pc = false;
        log("AV standby request: cancel PC wake work");
    }
    if op == 0x90 && f.len() == 3 && dst == cec.la {
        if src == c.tv {
            o.tv_power = Some(f[2]);
        }
        if src == c.avr {
            o.avr_power = Some(f[2]);
        }
    }
    if dst == 15 {
        let route = match op {
            0x82 if f.len() == 4 && src == c.tv && f[2..] == [0, 0]
                && c.tv_always_receiver => {
                log("TV root-source reply ignored: TV is configured to stay on receiver input");
                None
            }
            0x82 if f.len() == 4 => Some((u16::from_be_bytes([f[2], f[3]]), false)),
            0x81 if f.len() == 4 && (src == c.tv || src == c.avr) => {
                Some((u16::from_be_bytes([f[2], f[3]]), true))
            }
            0x80 if f.len() == 6 && (src == c.tv || src == c.avr) => {
                Some((u16::from_be_bytes([f[4], f[5]]), true))
            }
            // Set Stream Path is a request, not proof that the switch changed.
            0x86 if f.len() == 4 && src == c.tv => {
                let pa = u16::from_be_bytes([f[2], f[3]]);
                if pa != c.pc_pa {
                    if route_owner(pa, c.pc_pa) == Owner::Other {
                        m.route(Owner::Other, now);
                    } else {
                        m.forget_route();
                    }
                    o.route_pc = false;
                } else if m.state == State::PcActive {
                    cec.announce(c.pc_pa);
                }
                None
            }
            _ => None,
        };
        if let Some((pa, from_switch)) = route {
            if pa == c.pc_pa && from_switch {
                m.route(Owner::Pc, now);
                o.route_pc = true;
            } else if pa != c.pc_pa {
                if from_switch && route_owner(pa, c.pc_pa) == Owner::Unknown {
                    m.forget_route();
                } else {
                    m.route(Owner::Other, now);
                }
                o.route_pc = false;
            } else {
                m.forget_route();
                o.route_pc = false;
                log("conflicting Active Source at PC PA; ownership unknown");
            }
            log(format!("observed route={pa:04x} owner={:?}", m.owner));
        }
        if op == 0x85 && m.state == State::PcActive && m.owns_session() {
            cec.announce(c.pc_pa);
        }
    }
}

fn run() -> Result<(), String> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.iter().any(|s| s == "--help") {
        println!(concat!(
            "pi-cec [--config PATH] [--dry-run | --diagnostic | --commission | --check]\n",
            "Default: live (requires verified configuration). Diagnostics claim no CEC logical address.\n",
            "Commission: requires power/USB validation and proxy PA; standby disabled.\n",
            "Rust administration subcommands:\n  inspect\n  power-log [LOG] | power-report LOG\n",
            "  edid PI.bin [PC.bin OUTPUT.bin]\n  install | uninstall | enable-live\n",
            "  prepare-boot --quiet-cec | --low-power | --usb | --proxy-edid FILE --connector HDMI-A-1 | --remove\n",
            "  gadget start|stop | trace start|stop\nRun hardware administration on Pi, never on WSL."
        ));
        return Ok(());
    }
    let mut path = "/etc/pi-cec/pi-cec.conf".to_string();
    let (mut dry, mut commission, mut check) = (false, false, false);
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--config" => {
                i += 1;
                path = args.get(i).ok_or("missing config path")?.clone();
            }
            "--dry-run" | "--diagnostic" => dry = true,
            "--commission" => commission = true,
            "--check" => check = true,
            x => return Err(format!("unknown option {x}")),
        }
        i += 1;
    }
    if dry && commission {
        return Err("choose either diagnostic/dry-run or commission".into());
    }
    let mut c = Config::load(&path)?;
    if commission {
        log("COMMISSION: routing validation in progress; standby disabled");
        c.routing_verified = true;
        c.standby = false;
    }
    if !dry || check {
        c.live_check()?;
    }
    if check {
        println!("live configuration checks passed (not hardware validation)");
        return Ok(());
    }
    // Setup and read tracing in the same service mount namespace. ExecStartPre
    // mounts would not necessarily survive into systemd's ExecStart process.
    if c.usb_backend == "trace" {
        admin::setup_trace_for_daemon()?;
    }
    let start = Instant::now();
    let mut m = Machine::new(c.on_ms, c.off_ms, c.query_ms, c.standby);
    m.require_fresh = c.standby_policy == "fresh";
    m.allow_claimed = c.standby_policy == "session";
    m.startup_select_pc = c.startup_select_pc;
    let mut observed = Observed::default();
    let mut wake: Option<Wake> = None;
    let mut reader: Option<usb::UsbReader> = None;
    let mut cec: Option<Cec> = None;
    let (mut usb_retry, mut cec_retry, mut last_usb) = (0, 0, Usb::Unknown);
    log(format!("starting dry={dry}; startup standby suppressed; USB reason for host wake cannot be inferred"));
    log(format!("power_scope={} startup_select_pc={}", c.power_scope, c.startup_select_pc));
    loop {
        let now = start.elapsed().as_millis() as u64;
        if reader.is_none() && now >= usb_retry {
            match usb::UsbReader::open(&c.udc, &c.usb_backend, &c.trace_pipe, c.usb_verified) {
                Ok(r) => reader = Some(r),
                Err(e) => {
                    log(e);
                    usb_retry = now + 10000;
                }
            }
        }
        // Restart the process on trace failure: a blocked old reader must never
        // survive to consume an event belonging to a replacement reader.
        let usb = if let Some(r) = &mut reader {
            r.sample()?
        } else {
            Usb::Unknown
        };
        if usb != last_usb {
            log(format!("USB {last_usb:?} -> {usb:?}"));
            last_usb = usb;
        }
        if cec.is_none() && now >= cec_retry {
            match Cec::open(&c, dry) {
                Ok(a) => {
                    a.tx(15, &[0x85]);
                    cec = Some(a);
                    observed = Observed::default();
                    m.forget_route();
                }
                Err(e) => {
                    log(e);
                    cec_retry = now + 10000;
                }
            }
        }
        let mut lost = false;
        if let Some(a) = &cec {
            // Bound work per iteration so busy CEC buses cannot starve USB.
            for _ in 0..64 {
                match a.next() {
                    Ok(Some(CecEvent::Received(f))) => receive(&f, a, &c, &mut m, &mut observed, now),
                    Ok(Some(CecEvent::Transmitted(f, status))) => {
                        if status & 1 != 0 && f == [(a.la << 4) | 15, 0x82, (c.pc_pa >> 8) as u8, c.pc_pa as u8] {
                            m.claimed_pc(now);
                            if m.owner == Owner::PcClaimed {
                                log("CEC PC source claim held; actual switch route remains unconfirmed");
                            }
                        }
                    }
                    Ok(Some(CecEvent::Consumed)) => (),
                    Ok(None) => break,
                    Err(e) => {
                        log(e);
                        lost = true;
                        break;
                    }
                }
            }
        }
        if lost {
            cec = None;
            cec_retry = now + 5000;
            m.forget_route();
            observed = Observed::default();
            // Receiver power transitions can briefly drop HPD/CEC. Keep the
            // bounded wake transaction; fresh takeovers still cancel it via
            // generation, and ownership stays unknown until re-established.
            m.finish_reconcile();
        }
        let prev = m.state;
        for action in m.sample(usb, now) {
            log(format!(
                "action={action:?} owner={:?} observed={observed:?}",
                m.owner
            ));
            match action {
                Action::Wake=>{
                    observed=Observed::default();
                    wake=Some(Wake{generation:m.generation,attempt:0,stage:0,at:now});
                },
                Action::Reconcile=>{
                    observed=Observed::default();
                    if let Some(a)=&cec {
                        a.tx(15,&[0x85]);
                        a.tx(c.tv,&[0x8f]);
                        a.tx(c.avr,&[0x8f]);
                    }
                    wake=Some(Wake{generation:m.generation,attempt:0,stage:3,at:now+c.query_ms});
                    log("startup: checking existing AV route and power before selecting PC");
                },
                Action::QueryRoute=>{
                    if let Some(a)=&cec {a.tx(15,&[0x85]);}
                },
                Action::Standby=>{if let Some(a)=&cec {
                    // Directed only. Recheck ownership after processing RX.
                    // AVR standby can interrupt the downstream CEC link.
                    if m.owns_session() {a.tx(c.tv,&[0x36]);a.tx(c.avr,&[0x36]);}
                }},
                Action::SkipStandby=>log(format!("standby skipped: policy={} enabled={} owner={:?}; confirmation missing or another source",c.standby_policy,c.standby,m.owner)),
            }
        }
        if prev != m.state {
            log(format!(
                "state {prev:?} -> {:?}, desired_on={}",
                m.state, m.desired_on
            ));
        }
        if wake
            .as_ref()
            .map(|w| w.generation != m.generation || !m.desired_on)
            .unwrap_or(false)
        {
            wake = None;
        }
        if let (Some(w), Some(a)) = (&mut wake, &cec) {
            if now >= w.at {
                match w.stage {
                    0 => {
                        if observed.tv_power != Some(0) {
                            a.tx(c.tv, &[0x04]); // Image View On
                        }
                        if observed.avr_power != Some(0) {
                            a.tx(c.avr, &[0x44, 0x6d]);
                            a.tx(c.avr, &[0x45]); // discrete Power On Function
                        }
                        w.stage = 1;
                        w.at = now + if observed.tv_power == Some(0)
                            && observed.avr_power == Some(0) { 0 } else { c.warmup_ms };
                    }
                    1 => {
                        // Proxy PA is enforced in the native open path, never Pi's real PA.
                        a.announce(c.pc_pa);
                        a.tx(c.tv, &[0x8f]);
                        a.tx(c.avr, &[0x8f]);
                        w.stage = 2;
                        w.at = now + c.verify_ms;
                    }
                    3 => {
                        m.finish_reconcile();
                        if observed.route_pc && observed.tv_power == Some(0)
                            && observed.avr_power == Some(0) {
                            log("startup: AV already reports the PC route and power on; no selection needed");
                            wake = None;
                        } else {
                            log("startup: PC route/power needs reconciliation; selecting once with bounded retries");
                            m.forget_route();
                            w.stage = 0;
                            w.at = now;
                        }
                    }
                    _ => {
                        w.attempt += 1;
                        if (observed.route_pc || (m.allow_claimed && m.owns_session()))
                            && observed.tv_power == Some(0)
                            && observed.avr_power == Some(0)
                        {
                            log(format!("wake complete: both power-on reports received; route_observed={} source_owner={:?}; physical video validation is separate", observed.route_pc, m.owner));
                            wake = None;
                        } else if w.attempt >= c.attempts {
                            log(format!("wake retry budget exhausted; observed={observed:?}; routing unconfirmed"));
                            wake = None;
                        } else {
                            w.stage = 0;
                            w.at = now + (1000u64 << w.attempt);
                        }
                    }
                }
            }
        }
        // An absent adapter must not leave an old transition armed forever.
        if wake.as_ref().map(|w| now > w.at + 60000).unwrap_or(false) {
            log("wake expired while adapter unavailable; await next PC transition");
            wake = None;
            m.finish_reconcile();
        }
        thread::sleep(Duration::from_millis(100));
    }
}
fn main() {
    let args: Vec<_> = std::env::args().skip(1).collect();
    let result = admin::dispatch(&args).unwrap_or_else(run);
    if let Err(e) = result {
        log(e);
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn setup() -> (Config, Machine, Observed, Cec) {
        let mut c = Config::load("config/pi-cec.conf").unwrap();
        c.pc_pa = 0x1200;
        c.pi_pa = 0x1300;
        (
            c,
            Machine::new(100, 1000, 200, true),
            Observed::default(),
            Cec {
                fd: -1,
                la: 14,
                dry: true,
                pa: 0x1200,
            },
        )
    }
    #[test]
    fn self_announcement_is_not_proof() {
        let (c, mut m, mut o, a) = setup();
        receive(&[0xef, 0x82, 0x12, 0], &a, &c, &mut m, &mut o, 0);
        assert_eq!(m.owner, Owner::Unknown);
        assert!(!o.route_pc);
    }
    #[test]
    fn other_console_takes_over() {
        let (c, mut m, mut o, a) = setup();
        m.route(Owner::Pc, 0);
        receive(&[0x4f, 0x82, 0x14, 0], &a, &c, &mut m, &mut o, 100);
        assert_eq!(m.owner, Owner::Other);
        assert!(!m.desired_on);
    }
    #[test]
    fn stream_request_is_not_routing_proof() {
        let (c, mut m, mut o, a) = setup();
        receive(&[0x0f, 0x86, 0x12, 0], &a, &c, &mut m, &mut o, 0);
        assert_eq!(m.owner, Owner::Unknown);
    }
    #[test]
    fn receiver_route_is_positive_evidence() {
        let (c, mut m, mut o, a) = setup();
        receive(&[0x5f, 0x81, 0x12, 0], &a, &c, &mut m, &mut o, 0);
        assert_eq!(m.owner, Owner::Pc);
        assert!(o.route_pc);
    }
    #[test]
    fn unrelated_device_cannot_confirm_switch() {
        let (c, mut m, mut o, a) = setup();
        receive(&[0x4f, 0x81, 0x12, 0], &a, &c, &mut m, &mut o, 0);
        assert_eq!(m.owner, Owner::Unknown);
    }
    #[test]
    fn truncated_messages_are_ignored() {
        let (c, mut m, mut o, a) = setup();
        for f in [&[][..], &[0x5f][..], &[0x5f, 0x80, 0x12][..]] {
            receive(f, &a, &c, &mut m, &mut o, 0);
        }
        assert_eq!(m.owner, Owner::Unknown);
    }
    #[test]
    fn conflicting_proxy_source_is_unknown() {
        let (c, mut m, mut o, a) = setup();
        m.route(Owner::Pc, 0);
        receive(&[0x4f, 0x82, 0x12, 0], &a, &c, &mut m, &mut o, 100);
        assert_eq!(m.owner, Owner::Unknown);
    }
    #[test]
    fn receiver_branch_is_not_a_source() {
        assert_eq!(route_owner(0x1000, 0x1200), Owner::Unknown);
        assert_eq!(route_owner(0x1300, 0x1200), Owner::Other);
    }
    #[test]
    fn fixed_receiver_tv_root_reply_preserves_pc_but_console_takeover_wins() {
        let (mut c, mut m, mut o, a) = setup();
        c.tv_always_receiver = true;
        m.allow_claimed = true;
        m.state = State::PcActive;
        m.desired_on = true;
        m.claimed_pc(0);
        receive(&[0x0f, 0x82, 0, 0], &a, &c, &mut m, &mut o, 10);
        assert_eq!(m.owner, Owner::PcClaimed);
        assert!(!o.route_pc); // Still not proof of physical selection.
        receive(&[0x4f, 0x82, 0x13, 0], &a, &c, &mut m, &mut o, 20);
        assert_eq!(m.owner, Owner::Other);
        assert!(!m.desired_on);
    }
    #[test]
    fn default_policy_protects_tv_native_sources() {
        let (c, mut m, mut o, a) = setup();
        m.route(Owner::Pc, 0);
        receive(&[0x0f, 0x82, 0, 0], &a, &c, &mut m, &mut o, 10);
        assert_eq!(m.owner, Owner::Other);
    }
}
