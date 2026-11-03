//! Hardware-independent control and ownership policy. Time is monotonic ms.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Usb {
    Unknown,
    Inactive,
    Active,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum State {
    Unknown,
    PcInactive,
    PcActive,
    PendingOff,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Owner {
    Unknown,
    Pc,
    // CEC active-source session claim, distinct from observed switch routing.
    PcClaimed,
    Other,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    Wake,
    Reconcile,
    QueryRoute,
    Standby,
    SkipStandby,
}

pub struct Machine {
    pub state: State,
    pub owner: Owner,
    pub desired_on: bool,
    pub generation: u64,
    deadline: Option<u64>,
    candidate_since: Option<u64>,
    input: Usb,
    off_ms: u64,
    on_ms: u64,
    query_ms: u64,
    queried: bool,
    evidence: Option<u64>,
    pub standby_enabled: bool,
    pub require_fresh: bool,
    pub startup_select_pc: bool,
    pub reconciling: bool,
    pub allow_claimed: bool,
    startup_pending: bool,
}
impl Machine {
    pub fn new(on_ms: u64, off_ms: u64, query_ms: u64, standby: bool) -> Self {
        Self {
            state: State::Unknown,
            owner: Owner::Unknown,
            desired_on: false,
            generation: 0,
            deadline: None,
            candidate_since: None,
            input: Usb::Unknown,
            off_ms,
            on_ms,
            query_ms,
            queried: false,
            evidence: None,
            standby_enabled: standby,
            require_fresh: true,
            startup_select_pc: false,
            reconciling: false,
            allow_claimed: false,
            startup_pending: true,
        }
    }
    pub fn forget_route(&mut self) {
        self.owner = Owner::Unknown;
        self.evidence = None;
    }
    pub fn yield_session(&mut self) {
        self.forget_route();
        self.reconciling = false;
        self.desired_on = false;
        self.generation += 1;
    }
    pub fn route(&mut self, owner: Owner, now: u64) {
        self.owner = owner;
        self.evidence = Some(now);
        // A takeover permanently cancels retries for this PC session.
        if owner == Owner::Other && !self.reconciling {
            self.generation += 1;
            self.desired_on = false;
        }
    }
    pub fn finish_reconcile(&mut self) {
        self.reconciling = false;
    }
    pub fn claimed_pc(&mut self, now: u64) {
        if self.allow_claimed && self.state == State::PcActive && self.desired_on {
            if self.owner != Owner::Pc {
                self.owner = Owner::PcClaimed;
            }
            self.evidence = Some(now);
        }
    }
    pub fn owns_session(&self) -> bool {
        self.owner == Owner::Pc || (self.allow_claimed && self.owner == Owner::PcClaimed)
    }
    pub fn sample(&mut self, input: Usb, now: u64) -> Vec<Action> {
        let mut actions = Vec::new();
        if input != self.input {
            self.input = input;
            self.candidate_since = (input == Usb::Active).then_some(now);
        }
        if input == Usb::Unknown {
            self.reconciling = false;
            self.state = State::Unknown;
            self.deadline = None;
            self.desired_on = false;
            self.generation += 1;
            self.forget_route();
            return actions;
        }
        if input == Usb::Active {
            if self.state == State::PendingOff {
                // A bus reset must not steal the source back or repeat wake.
                self.state = State::PcActive;
                self.deadline = None;
                self.queried = false;
                return actions;
            }
            if self.state != State::PcActive
                && now.saturating_sub(self.candidate_since.unwrap_or(now)) >= self.on_ms
            {
                let startup = self.startup_pending && self.startup_select_pc;
                self.startup_pending = false;
                if self.state == State::Unknown && self.owner == Owner::Other && !startup {
                    self.state = State::PcActive;
                    self.desired_on = false;
                    return actions;
                }
                self.state = State::PcActive;
                self.desired_on = true;
                self.generation += 1;
                if startup {
                    self.reconciling = true;
                    actions.push(Action::Reconcile);
                } else {
                    self.forget_route();
                    actions.push(Action::Wake);
                }
            }
        } else {
            self.candidate_since = None;
            match self.state {
                State::PcActive => {
                    self.reconciling = false;
                    self.state = State::PendingOff;
                    self.deadline = Some(now + self.off_ms);
                    self.queried = false;
                    self.desired_on = false;
                    self.generation += 1;
                }
                State::Unknown => self.state = State::PcInactive, // never startup standby
                State::PendingOff => {
                    let deadline = self.deadline.unwrap();
                    if !self.queried && now >= deadline {
                        self.queried = true;
                        if self.require_fresh {
                            self.forget_route();
                        }
                        self.deadline = Some(now + self.query_ms);
                        actions.push(Action::QueryRoute);
                    } else if self.queried && now >= deadline {
                        self.state = State::PcInactive;
                        self.deadline = None;
                        let fresh = !self.require_fresh
                            || self
                                .evidence
                                .map(|t| t >= deadline - self.query_ms)
                                .unwrap_or(false);
                        actions.push(
                            if self.standby_enabled && self.owns_session() && fresh {
                                Action::Standby
                            } else {
                                Action::SkipStandby
                            },
                        );
                    }
                }
                _ => (),
            }
        }
        actions
    }
}

pub fn valid_pa(pa: u16) -> bool {
    if pa == 0 || pa == 0xffff {
        return false;
    }
    let mut zero = false;
    for shift in [12, 8, 4, 0] {
        let n = (pa >> shift) & 15;
        if zero && n != 0 {
            return false;
        }
        zero |= n == 0;
    }
    true
}
pub fn sibling_routes(a: u16, b: u16) -> bool {
    if !valid_pa(a) || !valid_pa(b) || a == b {
        return false;
    }
    fn parent(pa: u16) -> (u16, u8) {
        for shift in [0, 4, 8, 12] {
            if (pa >> shift) & 15 != 0 {
                return (pa & !(15 << shift), shift);
            }
        }
        (0, 0)
    }
    parent(a) == parent(b)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn machine() -> Machine {
        Machine::new(100, 1000, 200, true)
    }
    fn activate(m: &mut Machine) {
        m.sample(Usb::Active, 0);
        assert_eq!(m.sample(Usb::Active, 100), vec![Action::Wake]);
    }
    #[test]
    fn startup_inactive_is_silent() {
        let mut m = machine();
        assert!(m.sample(Usb::Inactive, 0).is_empty());
        assert!(m.sample(Usb::Inactive, 99999).is_empty());
    }
    #[test]
    fn active_idle_has_no_actions() {
        let mut m = machine();
        activate(&mut m);
        assert!(m.sample(Usb::Active, 10000000).is_empty());
    }
    #[test]
    fn reset_cancels_pending_without_stealing() {
        let mut m = machine();
        activate(&mut m);
        m.route(Owner::Other, 150);
        m.sample(Usb::Inactive, 200);
        assert!(m.sample(Usb::Active, 300).is_empty());
        assert_eq!(m.owner, Owner::Other);
    }
    #[test]
    fn standby_needs_fresh_confirmation() {
        let mut m = machine();
        activate(&mut m);
        m.route(Owner::Pc, 150);
        m.sample(Usb::Inactive, 200);
        assert_eq!(m.sample(Usb::Inactive, 1200), vec![Action::QueryRoute]);
        assert_eq!(m.sample(Usb::Inactive, 1400), vec![Action::SkipStandby]);
    }
    #[test]
    fn confirmed_pc_standby() {
        let mut m = machine();
        activate(&mut m);
        m.sample(Usb::Inactive, 200);
        m.sample(Usb::Inactive, 1200);
        m.route(Owner::Pc, 1250);
        assert_eq!(m.sample(Usb::Inactive, 1400), vec![Action::Standby]);
    }
    #[test]
    fn other_source_protected() {
        let mut m = machine();
        activate(&mut m);
        m.sample(Usb::Inactive, 200);
        m.sample(Usb::Inactive, 1200);
        m.route(Owner::Other, 1300);
        assert_eq!(m.sample(Usb::Inactive, 1400), vec![Action::SkipStandby]);
    }
    #[test]
    fn unknown_cancels_everything() {
        let mut m = machine();
        activate(&mut m);
        m.sample(Usb::Inactive, 200);
        m.sample(Usb::Unknown, 300);
        assert!(m.sample(Usb::Inactive, 10000).is_empty());
        assert_eq!(m.owner, Owner::Unknown);
    }
    #[test]
    fn resume_cancels_query_phase() {
        let mut m = machine();
        activate(&mut m);
        m.sample(Usb::Inactive, 200);
        m.sample(Usb::Inactive, 1200);
        assert!(m.sample(Usb::Active, 1250).is_empty());
        assert_eq!(m.state, State::PcActive);
    }
    #[test]
    fn disabled_standby() {
        let mut m = machine();
        m.standby_enabled = false;
        activate(&mut m);
        m.sample(Usb::Inactive, 200);
        m.sample(Usb::Inactive, 1200);
        m.route(Owner::Pc, 1300);
        assert_eq!(m.sample(Usb::Inactive, 1400), vec![Action::SkipStandby]);
    }
    #[test]
    fn debounce_and_cycles() {
        let mut m = machine();
        assert!(m.sample(Usb::Active, 0).is_empty());
        m.sample(Usb::Inactive, 50);
        assert!(m.sample(Usb::Active, 60).is_empty());
        assert_eq!(m.sample(Usb::Active, 160), vec![Action::Wake]);
        m.sample(Usb::Inactive, 200);
        m.sample(Usb::Inactive, 1200);
        m.sample(Usb::Inactive, 1400);
        m.sample(Usb::Active, 2000);
        assert_eq!(m.sample(Usb::Active, 2100), vec![Action::Wake]);
    }
    #[test]
    fn addresses() {
        assert!(sibling_routes(0x1200, 0x1300));
        assert!(!sibling_routes(0x1200, 0x2100));
        assert!(!sibling_routes(0x1200, 0x1230));
        assert!(!valid_pa(0x1020));
        assert!(!valid_pa(0xffff));
    }
    #[test]
    fn restart_respects_observed_other_source() {
        let mut m = machine();
        m.sample(Usb::Active, 0);
        m.route(Owner::Other, 50);
        assert!(m.sample(Usb::Active, 100).is_empty());
        assert_eq!(m.state, State::PcActive);
    }
    #[test]
    fn tracked_policy_retains_confirmed_ownership() {
        let mut m = machine();
        m.require_fresh = false;
        activate(&mut m);
        m.route(Owner::Pc, 150);
        m.sample(Usb::Inactive, 200);
        m.sample(Usb::Inactive, 1200);
        assert_eq!(m.sample(Usb::Inactive, 1400), vec![Action::Standby]);
    }
    #[test]
    fn tracked_policy_never_infers_ownership_from_wake() {
        let mut m = machine();
        m.require_fresh = false;
        activate(&mut m);
        m.sample(Usb::Inactive, 200);
        m.sample(Usb::Inactive, 1200);
        assert_eq!(m.sample(Usb::Inactive, 1400), vec![Action::SkipStandby]);
    }
    #[test]
    fn cold_start_checks_existing_route_before_selecting_pc() {
        let mut m = machine();
        m.startup_select_pc = true;
        m.sample(Usb::Active, 0);
        m.route(Owner::Other, 50);
        assert_eq!(m.sample(Usb::Active, 100), vec![Action::Reconcile]);
        let generation = m.generation;
        // An answer identifying the old source is baseline, not a new takeover.
        m.route(Owner::Other, 150);
        assert_eq!(m.generation, generation);
        assert!(m.desired_on);
        m.finish_reconcile();
        m.route(Owner::Other, 200);
        assert!(!m.desired_on);
        assert!(m.generation > generation);
    }
    #[test]
    fn suspend_cancels_startup_reconciliation() {
        let mut m = machine();
        m.startup_select_pc = true;
        m.sample(Usb::Active, 0);
        m.sample(Usb::Active, 100);
        m.sample(Usb::Inactive, 150);
        assert!(!m.reconciling);
        assert!(!m.desired_on);
        assert_eq!(m.state, State::PendingOff);
    }
    #[test]
    fn cold_start_without_active_bus_never_selects_or_standbys() {
        let mut m = machine();
        m.startup_select_pc = true;
        assert!(m.sample(Usb::Unknown, 0).is_empty());
        assert!(m.sample(Usb::Inactive, 100).is_empty());
        assert!(m.sample(Usb::Inactive, 10000).is_empty());
    }
    #[test]
    fn cec_session_claim_allows_standby_until_another_source_takes_over() {
        let mut m = machine();
        m.allow_claimed = true;
        m.require_fresh = false;
        activate(&mut m);
        m.claimed_pc(110);
        assert_eq!(m.owner, Owner::PcClaimed);
        m.sample(Usb::Inactive, 200);
        m.sample(Usb::Inactive, 1200);
        assert_eq!(m.sample(Usb::Inactive, 1400), vec![Action::Standby]);

        m.sample(Usb::Active, 2000);
        m.sample(Usb::Active, 2100);
        m.claimed_pc(2110);
        m.route(Owner::Other, 2200);
        m.claimed_pc(2300); // Late TX completion cannot undo a takeover.
        assert_eq!(m.owner, Owner::Other);
        m.sample(Usb::Inactive, 2400);
        m.sample(Usb::Inactive, 3400);
        assert_eq!(m.sample(Usb::Inactive, 3600), vec![Action::SkipStandby]);
    }
    #[test]
    fn strict_policy_and_adapter_loss_do_not_trust_session_claims() {
        let mut m = machine();
        activate(&mut m);
        m.claimed_pc(110);
        assert_eq!(m.owner, Owner::Unknown);
        m.allow_claimed = true;
        m.claimed_pc(120);
        let generation = m.generation;
        m.forget_route();
        assert!(!m.owns_session());
        assert!(m.desired_on); // An in-progress wake can recover from AVR HPD loss.
        assert_eq!(m.generation, generation);
    }
    #[test]
    fn startup_reconciles_after_delayed_usb_enumeration() {
        let mut m = machine();
        m.startup_select_pc = true;
        m.sample(Usb::Inactive, 0);
        m.sample(Usb::Active, 1000);
        assert_eq!(m.sample(Usb::Active, 1100), vec![Action::Reconcile]);
        m.finish_reconcile();
        m.sample(Usb::Inactive, 2000);
        m.sample(Usb::Inactive, 3000);
        m.sample(Usb::Inactive, 3200);
        m.sample(Usb::Active, 4000);
        assert_eq!(m.sample(Usb::Active, 4100), vec![Action::Wake]);
    }
}
