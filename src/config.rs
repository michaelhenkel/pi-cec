use std::{collections::BTreeMap, fs};
pub struct Config {
    pub adapter: String,
    pub udc: String,
    pub usb_backend: String,
    pub trace_pipe: String,
    pub pc_pa: u16,
    pub pi_pa: u16,
    pub tv: u8,
    pub avr: u8,
    pub on_ms: u64,
    pub off_ms: u64,
    pub query_ms: u64,
    pub warmup_ms: u64,
    pub verify_ms: u64,
    pub attempts: u32,
    pub standby: bool,
    pub power_verified: bool,
    pub power_scope: String,
    pub startup_select_pc: bool,
    pub tv_always_receiver: bool,
    pub usb_verified: bool,
    pub routing_verified: bool,
    pub playback: bool,
    pub verified_kernel: String,
    pub standby_policy: String,
}
impl Config {
    pub fn load(path: &str) -> Result<Self, String> {
        let text = fs::read_to_string(path).map_err(|e| e.to_string())?;
        let mut map = BTreeMap::new();
        for (i, line) in text.lines().enumerate() {
            let line = line.split('#').next().unwrap().trim();
            if line.is_empty() {
                continue;
            }
            let (k, v) = line
                .split_once('=')
                .ok_or(format!("line {}: expected key=value", i + 1))?;
            if map
                .insert(k.trim().to_string(), v.trim().to_string())
                .is_some()
            {
                return Err(format!("duplicate key {k}"));
            }
        }
        let mut take = |k: &str, default: &str| map.remove(k).unwrap_or_else(|| default.into());
        fn num<T: std::str::FromStr>(s: String) -> Result<T, String> {
            s.parse().map_err(|_| format!("invalid value {s}"))
        }
        fn pa(s: String) -> Result<u16, String> {
            u16::from_str_radix(s.trim_start_matches("0x"), 16)
                .map_err(|_| format!("invalid PA {s}"))
        }
        let c = Self {
            adapter: take("adapter", ""),
            udc: take("udc", ""),
            usb_backend: take("usb_backend", "trace"),
            trace_pipe: take(
                "trace_pipe",
                "/sys/kernel/tracing/instances/pi-cec/trace_pipe",
            ),
            pc_pa: pa(take("pc_pa", "ffff"))?,
            pi_pa: pa(take("pi_pa", "ffff"))?,
            tv: num(take("tv_la", "0"))?,
            avr: num(take("avr_la", "5"))?,
            on_ms: num(take("on_debounce_ms", "1500"))?,
            off_ms: num(take("off_debounce_ms", "15000"))?,
            query_ms: num(take("route_query_ms", "3000"))?,
            warmup_ms: num(take("warmup_ms", "12000"))?,
            verify_ms: num(take("verify_ms", "4000"))?,
            attempts: num(take("wake_attempts", "3"))?,
            standby: num(take("standby", "false"))?,
            power_verified: num(take("power_verified", "false"))?,
            power_scope: take("power_scope", "continuous"),
            startup_select_pc: num(take("startup_select_pc", "false"))?,
            tv_always_receiver: num(take("tv_always_receiver", "false"))?,
            usb_verified: num(take("usb_verified", "false"))?,
            routing_verified: num(take("routing_verified", "false"))?,
            playback: num(take("playback_identity", "false"))?,
            verified_kernel: take("verified_kernel", ""),
            standby_policy: take("standby_policy", "fresh"),
        };
        if !map.is_empty() {
            return Err(format!("unknown keys: {:?}", map.keys()));
        }
        if c.tv > 14
            || c.avr > 14
            || c.tv == c.avr
            || c.attempts == 0
            || c.attempts > 5
            || c.on_ms < 100
            || c.off_ms < 1000
            || c.query_ms < 500
            || c.warmup_ms < 1000
            || c.verify_ms < 500
        {
            return Err("unsafe timing/address settings".into());
        }
        if c.on_ms > 60000
            || c.off_ms > 600000
            || c.query_ms > 30000
            || c.warmup_ms > 120000
            || c.verify_ms > 60000
        {
            return Err("timing exceeds supported bounds".into());
        }
        if !["trace", "sysfs"].contains(&c.usb_backend.as_str()) {
            return Err("usb_backend must be trace or sysfs".into());
        }
        if !["fresh", "tracked", "session"].contains(&c.standby_policy.as_str()) {
            return Err("standby_policy must be fresh, tracked or session".into());
        }
        if !["continuous", "sleep"].contains(&c.power_scope.as_str()) {
            return Err("power_scope must be continuous or sleep".into());
        }
        Ok(c)
    }
    pub fn live_check(&self) -> Result<(), String> {
        if !self.power_verified || !self.usb_verified || !self.routing_verified {
            return Err(
                "live mode requires measured power_verified, usb_verified and routing_verified"
                    .into(),
            );
        }
        let kernel = fs::read_to_string("/proc/sys/kernel/osrelease").map_err(|e| e.to_string())?;
        if self.verified_kernel != kernel.trim() {
            return Err("kernel changed or unverified: remeasure USB events, set verified_kernel to uname -r".into());
        }
        if !pi_cec::sibling_routes(self.pc_pa, self.pi_pa) {
            return Err(
                "pc_pa and original pi_pa must be distinct sibling HDMI paths measured from EDID"
                    .into(),
            );
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn shipped_config_is_fail_closed() {
        let c = Config::load("config/pi-cec.conf").unwrap();
        assert!(c.live_check().is_err());
        assert!(!c.standby);
    }
}
