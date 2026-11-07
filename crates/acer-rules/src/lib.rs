//! Pure power-transition rules. The caller owns all I/O and dispatch authorization.
pub mod thermal;
use acer_protocol::{Capabilities, CapabilityStatus, Command, Profile, Snapshot};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RuleConfig {
    pub enabled: bool,
    pub ac_profile: Option<Profile>,
    pub dc_profile: Option<Profile>,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PowerSource { Ac, Dc }
#[derive(Debug, Clone, Copy)]
pub struct Observation { pub power: Option<PowerSource>, pub profile: Option<Profile> }
impl Observation {
    /// Pass only a newly received real snapshot, never cached data after wake.
    pub fn from_snapshot(snapshot: &Snapshot) -> Self {
        Self { power: snapshot.ac_online.map(|v|if v { PowerSource::Ac } else { PowerSource::Dc }), profile: snapshot.profile }
    }
}
/// Native, verified per-power restrictions. Never populate this from rule/UI JSON.
/// An empty list blocks all targets until their device/power support is established.
#[derive(Debug, Clone, Default)]
pub struct VerifiedTargets { pub ac: Vec<Profile>, pub dc: Vec<Profile> }
impl VerifiedTargets {
    fn allows(&self, power: PowerSource, target: Profile) -> bool {
        match power { PowerSource::Ac => &self.ac, PowerSource::Dc => &self.dc }.contains(&target)
    }
}
#[derive(Debug, Clone)]
pub struct RuleAction { pub id: u64, pub power: PowerSource, pub target: Profile, pub command: Command }
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reason { Disabled, UnknownPower, Baseline, Unchanged, NoTarget, CapabilityBlocked, TargetBlocked, AlreadySelected, Busy, Emit, WakeRefresh, Reconfigured, IdExhausted }
#[derive(Debug, Clone)]
pub struct Decision { pub action: Option<RuleAction>, pub refresh_requested: bool, pub reason: Reason }
impl Decision {
    fn idle(reason: Reason) -> Self { Self { action: None, refresh_requested: false, reason } }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Completion { Applied, Failed, Unconfirmed }
#[derive(Debug)]
pub struct RuleEngine {
    config: RuleConfig,
    baseline: Option<PowerSource>,
    pending: Option<RuleAction>,
    next_id: u64,
    last_completion: Option<(u64,Completion)>,
}
impl Default for RuleEngine { fn default() -> Self { Self::new(RuleConfig::default()) } }
impl RuleEngine {
    /// Construction performs no I/O and does not emit a command, even if enabled.
    pub fn new(config: RuleConfig) -> Self { Self { config, baseline: None, pending: None, next_id: 1, last_completion: None } }
    pub fn config(&self) -> &RuleConfig { &self.config }
    pub fn reconfigure(&mut self, config: RuleConfig) -> Decision {
        self.config=config; self.baseline=None; self.pending=None;
        Decision { action:None, refresh_requested:true, reason:Reason::Reconfigured }
    }
    /// Invalidates queued actions; an operation already dispatched cannot be cancelled here.
    pub fn on_wake(&mut self) -> Decision {
        self.baseline=None; self.pending=None;
        Decision { action:None, refresh_requested:true, reason:Reason::WakeRefresh }
    }
    pub fn observe_snapshot(&mut self, snapshot: &Snapshot, capabilities: &Capabilities, targets: &VerifiedTargets) -> Decision {
        self.observe(Observation::from_snapshot(snapshot),capabilities,targets)
    }
    /// One observation per newly completed read/event, serially delivered by the core.
    pub fn observe(&mut self, observation: Observation, capabilities: &Capabilities, targets: &VerifiedTargets) -> Decision {
        let Some(power)=observation.power else {
            self.baseline=None; self.pending=None;
            return Decision::idle(Reason::UnknownPower);
        };
        let previous=self.baseline.replace(power);
        if !self.config.enabled { return Decision::idle(Reason::Disabled); }
        let Some(previous)=previous else { return Decision::idle(Reason::Baseline); };
        if previous==power { return Decision::idle(Reason::Unchanged); }
        // Consume this transition even if busy/blocked; do not queue or retry it later.
        if self.pending.is_some() { return Decision::idle(Reason::Busy); }
        let Some(target)=(match power { PowerSource::Ac=>self.config.ac_profile,PowerSource::Dc=>self.config.dc_profile }) else { return Decision::idle(Reason::NoTarget); };
        if !capabilities.matched_model || !capabilities.items.iter().any(|v|v.id=="profile" && v.writable && v.status==CapabilityStatus::Verified) {
            return Decision::idle(Reason::CapabilityBlocked);
        }
        if !targets.allows(power,target) { return Decision::idle(Reason::TargetBlocked); }
        if observation.profile==Some(target) { return Decision::idle(Reason::AlreadySelected); }
        let Some(next)=self.next_id.checked_add(1) else { return Decision::idle(Reason::IdExhausted); };
        let action=RuleAction { id:self.next_id,power,target,command:Command::SetProfile {profile:target} };
        self.next_id=next; self.pending=Some(action.clone());
        Decision { action:Some(action),refresh_requested:false,reason:Reason::Emit }
    }
    /// Core rechecks before dispatch, alongside fresh capability/AC/DC restrictions.
    pub fn is_current(&self, id: u64) -> bool {
        self.config.enabled && self.pending.as_ref().is_some_and(|a| a.id==id && self.baseline==Some(a.power))
    }
    /// Completes once. All outcomes consume the action; no automatic retry is scheduled.
    pub fn finish(&mut self, id: u64, outcome: Completion) -> bool {
        if !self.pending.as_ref().is_some_and(|a|a.id==id) { return false; }
        self.pending=None; self.last_completion=Some((id,outcome)); true
    }
    pub fn last_completion(&self) -> Option<(u64,Completion)> { self.last_completion }
}

#[cfg(test)] mod tests {
    use super::*;
    use acer_protocol::Capability;
    fn config()->RuleConfig { RuleConfig {enabled:true,ac_profile:Some(Profile::Performance),dc_profile:Some(Profile::Quiet)} }
    fn caps()->Capabilities { Capabilities {model:"PHN16-71".into(),bios:"V1.18".into(),matched_model:true,items:vec![Capability {id:"profile".into(),readable:true,writable:true,status:CapabilityStatus::Verified,reason:"test fixture".into()}]} }
    fn targets()->VerifiedTargets { VerifiedTargets {ac:vec![Profile::Performance],dc:vec![Profile::Quiet]} }
    fn observation(power:Option<PowerSource>)->Observation { Observation {power,profile:Some(Profile::Balanced)} }
    fn feed(engine:&mut RuleEngine,power:Option<PowerSource>)->Decision { engine.observe(observation(power),&caps(),&targets()) }
    #[test] fn defaults_and_first_read_never_emit() {
        let mut e=RuleEngine::default(); assert_eq!(feed(&mut e,Some(PowerSource::Ac)).reason,Reason::Disabled);
        assert_eq!(feed(&mut e,Some(PowerSource::Dc)).reason,Reason::Disabled);
        let mut e=RuleEngine::new(config()); assert_eq!(feed(&mut e,Some(PowerSource::Ac)).reason,Reason::Baseline);
    }
    #[test] fn confirmed_transition_emits_once_and_failure_never_retries() {
        let mut e=RuleEngine::new(config());feed(&mut e,Some(PowerSource::Ac));
        let action=feed(&mut e,Some(PowerSource::Dc)).action.unwrap();
        assert!(matches!(action.command,Command::SetProfile{profile:Profile::Quiet}));
        assert!(e.is_current(action.id)); assert!(e.finish(action.id,Completion::Failed));
        assert!(!e.finish(action.id,Completion::Failed));
        for _ in 0..5 { assert_eq!(feed(&mut e,Some(PowerSource::Dc)).reason,Reason::Unchanged); }
        assert!(feed(&mut e,Some(PowerSource::Ac)).action.unwrap().id>action.id);
    }
    #[test] fn unknown_breaks_transition_chain_and_invalidates_pending() {
        let mut e=RuleEngine::new(config());feed(&mut e,Some(PowerSource::Ac));
        let action=feed(&mut e,Some(PowerSource::Dc)).action.unwrap();
        assert_eq!(feed(&mut e,None).reason,Reason::UnknownPower);assert!(!e.is_current(action.id));
        assert!(!e.finish(action.id,Completion::Applied));
        assert_eq!(feed(&mut e,Some(PowerSource::Ac)).reason,Reason::Baseline);
    }
    #[test] fn capability_and_power_target_both_gate_writes() {
        let mut e=RuleEngine::new(config());feed(&mut e,Some(PowerSource::Ac));
        let mut c=caps();c.items[0].writable=false;
        assert_eq!(e.observe(observation(Some(PowerSource::Dc)),&c,&targets()).reason,Reason::CapabilityBlocked);
        assert!(feed(&mut e,Some(PowerSource::Dc)).action.is_none());
        assert_eq!(e.observe(observation(Some(PowerSource::Ac)),&caps(),&VerifiedTargets::default()).reason,Reason::TargetBlocked);
    }
    #[test] fn wake_and_reconfiguration_require_fresh_baseline() {
        let mut e=RuleEngine::new(config());feed(&mut e,Some(PowerSource::Ac));
        let action=feed(&mut e,Some(PowerSource::Dc)).action.unwrap();
        let wake=e.on_wake();assert!(wake.refresh_requested);assert!(wake.action.is_none());assert!(!e.is_current(action.id));
        assert_eq!(feed(&mut e,Some(PowerSource::Ac)).reason,Reason::Baseline);
        assert!(e.reconfigure(config()).action.is_none());assert_eq!(feed(&mut e,Some(PowerSource::Dc)).reason,Reason::Baseline);
    }
    #[test] fn busy_does_not_queue_and_already_selected_is_skipped() {
        let mut e=RuleEngine::new(config());feed(&mut e,Some(PowerSource::Ac));
        let action=feed(&mut e,Some(PowerSource::Dc)).action.unwrap();
        assert_eq!(feed(&mut e,Some(PowerSource::Ac)).reason,Reason::Busy);assert!(!e.is_current(action.id));
        assert!(e.finish(action.id,Completion::Unconfirmed));assert!(feed(&mut e,Some(PowerSource::Ac)).action.is_none());
        assert_eq!(e.observe(Observation{power:Some(PowerSource::Dc),profile:Some(Profile::Quiet)},&caps(),&targets()).reason,Reason::AlreadySelected);
    }
    #[test] fn optional_target_and_id_exhaustion_do_not_emit() {
        let mut cfg=config();cfg.dc_profile=None;let mut e=RuleEngine::new(cfg);feed(&mut e,Some(PowerSource::Ac));
        assert_eq!(feed(&mut e,Some(PowerSource::Dc)).reason,Reason::NoTarget);
        e.next_id=u64::MAX;
        assert_eq!(feed(&mut e,Some(PowerSource::Ac)).reason,Reason::IdExhausted);
    }
}
