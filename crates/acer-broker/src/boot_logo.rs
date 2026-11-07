//! Helper-session boot-logo plans. Image source paths must originate from the
//! native file chooser; never expose Image preparation as an arbitrary WebView
//! path command. A plan ID refers to this authenticated session's held object,
//! not an independent authorization secret or a serialized transaction.
use acer_boot_logo::native_transaction::{self, NativePlan, NativeReport};
use acer_protocol::{DeviceError, ErrorKind};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{io, path::PathBuf, sync::{Mutex, MutexGuard}, time::{Duration, Instant}};

const PLAN_TTL:Duration=Duration::from_secs(5*60);

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag="kind", rename_all="snake_case", deny_unknown_fields)]
pub(crate) enum BootLogoAction {
    Image { source:PathBuf },
    // An empty struct variant keeps unknown-field rejection effective for this
    // internally tagged no-argument action, unlike a serde unit variant.
    FactoryReset {},
    Restore { backup_id:String },
}

// The default and only production instantiation owns a real NativePlan. The
// private type parameter permits drop/expiry tests without preparing EFI plans.
struct Pending<P=NativePlan> { plan:P, id:String, created:Instant }

pub(crate) struct BootLogoManager { pending:Mutex<Option<Pending>> }
impl Default for BootLogoManager { fn default()->Self { Self::new() } }

fn failure(kind:ErrorKind,message:impl ToString)->DeviceError { DeviceError{kind,message:message.to_string()} }
fn native_error(error:io::Error)->DeviceError {
    let kind=match error.kind() {
        io::ErrorKind::PermissionDenied=>ErrorKind::PermissionDenied,
        io::ErrorKind::InvalidInput=>ErrorKind::InvalidArgument,
        io::ErrorKind::InvalidData=>ErrorKind::Protocol,
        io::ErrorKind::Unsupported=>ErrorKind::Unsupported,
        _=>ErrorKind::Transport,
    };
    failure(kind,error)
}
fn json_error(error:serde_json::Error)->DeviceError { failure(ErrorKind::Protocol,error) }
fn expires_in(created:Instant,now:Instant)->Duration { PLAN_TTL.saturating_sub(now.saturating_duration_since(created)) }
fn expire_pending<P>(pending:&mut Option<Pending<P>>,now:Instant)->bool {
    if pending.as_ref().is_some_and(|p|now.saturating_duration_since(p.created)>=PLAN_TTL) {
        // Drop the owned NativePlan, including its pinned ancestor handles.
        drop(pending.take());true
    } else { false }
}
fn take_matching<P>(pending:&mut Option<Pending<P>>,id:&str,now:Instant)->Result<Pending<P>,DeviceError> {
    expire_pending(pending,now);
    if !pending.as_ref().is_some_and(|p|p.id==id) {
        return Err(failure(ErrorKind::InvalidArgument,"No matching unexpired boot-logo plan in this helper session"));
    }
    // Check before take: a wrong ID cannot consume a valid plan.
    pending.take().ok_or_else(||failure(ErrorKind::Protocol,"Boot-logo pending plan disappeared"))
}
fn prepared_json(pending:&Pending,now:Instant)->Result<Value,DeviceError> {
    let review=serde_json::to_value(pending.plan.preview()).map_err(json_error)?;
    Ok(json!({"status":"prepared","plan_id":pending.id,"backup_id":pending.id,
        "review":review,"expires_seconds":expires_in(pending.created,now).as_secs()}))
}
fn commit_status(report:&NativeReport)->(&'static str,bool,bool) {
    let transaction=&report.transaction;
    let rollback_complete=transaction.rollback.len()==4 && transaction.rollback.iter().enumerate()
        .all(|(index,(slot,result))|*slot==index && result.is_ok());
    if report.journal_error.is_some() { return ("recovery_required",false,rollback_complete); }
    if transaction.applied && transaction.failures.is_empty() && transaction.rollback.is_empty() {
        ("applied",true,false)
    } else if !transaction.applied && rollback_complete {
        ("rolled_back",false,true)
    } else { ("recovery_required",false,rollback_complete) }
}
fn completed_json(plan_id:&str,review:Value,report:NativeReport)->Value {
    let(status,success,rollback_complete)=commit_status(&report);
    let backup_id=report.backup_id.clone();
    json!({"status":status,"success":success,"plan_id":plan_id,"backup_id":backup_id,
        "review":review,"expires_seconds":0,"plan_consumed":true,
        "commit":{"report":report,"error":null,"rollback_complete":rollback_complete}})
}
fn execution_error_json(plan_id:&str,review:Value,error:io::Error)->Value {
    // The one-use native plan is already consumed. An I/O error must retain
    // the backup reference and never be reported as a successful transaction.
    json!({"status":"recovery_required","success":false,"plan_id":plan_id,"backup_id":plan_id,
        "review":review,"expires_seconds":0,"plan_consumed":true,
        "commit":{"report":null,"error":native_error(error),"rollback_complete":false}})
}

impl BootLogoManager {
    pub(crate) fn new()->Self { Self{pending:Mutex::new(None)} }
    fn lock(&self)->Result<MutexGuard<'_,Option<Pending>>,DeviceError> {
        self.pending.lock().map_err(|_|failure(ErrorKind::WorkerStopped,"Boot-logo plan manager lock is poisoned"))
    }
    /// Call at the start of every helper request-loop iteration. No directory
    /// creation, journal write or official service query occurs during expiry.
    pub(crate) fn expire(&self)->Result<bool,DeviceError> {
        let mut pending=self.lock()?;
        Ok(expire_pending(&mut pending,Instant::now()))
    }
    /// Read-only native discovery and backup inventory; no prepare call or
    /// factory getter is used here, including when the backup root is missing.
    pub(crate) fn query(&self)->Result<Value,DeviceError> {
        self.expire()?;
        let inspection=native_transaction::inspect_current().map_err(native_error)?;
        let backups=native_transaction::list_backups().map_err(native_error)?;
        let mut pending=self.lock()?;let now=Instant::now();expire_pending(&mut pending,now);
        let pending=match pending.as_ref() { Some(plan)=>prepared_json(plan,now)?,None=>Value::Null };
        Ok(json!({"status":"inspected","inspection":inspection,"backups":backups,"pending":pending}))
    }
    /// Native image selection or a fixed factory/backup action only. Never
    /// accept caller targets, manifests, image bytes or a deserialized plan.
    pub(crate) fn prepare(&self,action:BootLogoAction)->Result<Value,DeviceError> {
        let mut pending=self.lock()?;expire_pending(&mut pending,Instant::now());
        if pending.is_some() {
            return Err(failure(ErrorKind::InvalidArgument,"Discard the existing boot-logo plan before preparing another"));
        }
        let plan=match action {
            BootLogoAction::Image{source}=>{
                if !source.is_absolute() { return Err(failure(ErrorKind::InvalidArgument,"Native file chooser must provide an absolute image source")); }
                native_transaction::prepare_image(&source)
            }
            BootLogoAction::FactoryReset{}=>native_transaction::prepare_factory_reset(),
            BootLogoAction::Restore{backup_id}=>native_transaction::prepare_backup_restore(&backup_id),
        }.map_err(native_error)?;
        let id=plan.backup_id().to_string();
        let prepared=Pending{plan,id,created:Instant::now()};
        let review=prepared_json(&prepared,prepared.created)?;
        *pending=Some(prepared);Ok(review)
    }
    /// Match and take exactly once while holding the manager lock. The lock is
    /// retained through execution so another prepare cannot race the commit.
    pub(crate) fn commit(&self,plan_id:&str)->Result<Value,DeviceError> {
        let mut slot=self.lock()?;expire_pending(&mut slot,Instant::now());
        let review=match slot.as_ref().filter(|p|p.id==plan_id) {
            Some(pending)=>serde_json::to_value(pending.plan.preview()).map_err(json_error)?,
            None=>return Err(failure(ErrorKind::InvalidArgument,"No matching unexpired boot-logo plan in this helper session")),
        };
        let pending=take_matching(&mut slot,plan_id,Instant::now())?;
        let id=pending.id;
        let result=match pending.plan.execute() {
            Ok(report)=>completed_json(&id,review,report),
            Err(error)=>execution_error_json(&id,review,error),
        };
        drop(slot);Ok(result)
    }
    pub(crate) fn discard(&self,plan_id:&str)->Result<Value,DeviceError> {
        let mut slot=self.lock()?;
        let pending=take_matching(&mut slot,plan_id,Instant::now())?;
        let id=pending.id;
        drop(pending.plan);
        Ok(json!({"status":"discarded","plan_id":id,"backup_id":id,"expires_seconds":0,"plan_consumed":true}))
    }
}

#[cfg(test)]mod tests {
    use super::*;
    use acer_boot_logo::Report;
    use std::sync::{Arc,atomic::{AtomicUsize,Ordering}};
    struct DropProbe(Arc<AtomicUsize>);
    impl Drop for DropProbe { fn drop(&mut self){self.0.fetch_add(1,Ordering::SeqCst);} }
    fn fake_pending(id:&str,created:Instant,drops:&Arc<AtomicUsize>)->Option<Pending<DropProbe>> {
        Some(Pending{plan:DropProbe(drops.clone()),id:id.into(),created})
    }
    fn report(applied:bool,failures:Vec<String>,rollback:Vec<(usize,Result<(),String>)>,journal_error:Option<String>)->NativeReport {
        NativeReport{backup_id:"123-456-1".into(),transaction:Report{applied,failures,rollback},journal_error}
    }
    #[test]fn actions_are_closed_and_factory_rejects_unknown_fields() {
        for input in [r#"{"kind":"image","source":"C:\\selected\\logo.gif"}"#,
            r#"{"kind":"factory_reset"}"#,r#"{"kind":"restore","backup_id":"123-456-1"}"#] {
            assert!(serde_json::from_str::<BootLogoAction>(input).is_ok());
        }
        for input in [r#"{"kind":"factory_reset","targets":[]}"#,
            r#"{"kind":"image","source":"logo.gif","manifest":{}}"#,
            r#"{"kind":"restore","backup_id":"123-456-1","imagebytes":[1]}"#,
            r#"{"kind":"commit","plan_id":"123-456-1"}"#,
            r#"{"kind":"arbitrary","source":"x"}"#] {
            assert!(serde_json::from_str::<BootLogoAction>(input).is_err(),"accepted {input}");
        }
    }
    #[test]fn wrong_id_never_consumes_valid_plan_and_correct_take_is_one_use() {
        let now=Instant::now();let drops=Arc::new(AtomicUsize::new(0));let mut slot=fake_pending("123-456-1",now,&drops);
        assert!(take_matching(&mut slot,"wrong",now).is_err());assert!(slot.is_some());assert_eq!(drops.load(Ordering::SeqCst),0);
        let pending=take_matching(&mut slot,"123-456-1",now).unwrap();assert!(slot.is_none());
        assert!(take_matching(&mut slot,"123-456-1",now).is_err());assert_eq!(drops.load(Ordering::SeqCst),0);
        drop(pending);assert_eq!(drops.load(Ordering::SeqCst),1);
    }
    #[test]fn expiry_boundary_drops_owned_resource_without_native_prepare() {
        let created=Instant::now();let drops=Arc::new(AtomicUsize::new(0));let mut slot=fake_pending("id",created,&drops);
        assert!(!expire_pending(&mut slot,created+PLAN_TTL-Duration::from_millis(1)));
        assert_eq!(drops.load(Ordering::SeqCst),0);
        assert!(expire_pending(&mut slot,created+PLAN_TTL));assert!(slot.is_none());assert_eq!(drops.load(Ordering::SeqCst),1);
        assert!(!expire_pending(&mut slot,created+PLAN_TTL+Duration::from_secs(1)));
    }
    #[test]fn expired_commit_reference_releases_resource_instead_of_returning_it() {
        let created=Instant::now();let drops=Arc::new(AtomicUsize::new(0));let mut slot=fake_pending("id",created,&drops);
        assert!(take_matching(&mut slot,"id",created+PLAN_TTL).is_err());
        assert!(slot.is_none());assert_eq!(drops.load(Ordering::SeqCst),1);
    }
    #[test]fn only_clean_applied_transaction_is_success() {
        let clean=report(true,vec![],vec![],None);assert_eq!(commit_status(&clean),("applied",true,false));
        let journal=report(true,vec![],vec![],Some("journal failed".into()));
        assert_eq!(commit_status(&journal),("recovery_required",false,false));
        let contradictory=report(true,vec!["failure".into()],vec![],None);
        assert_eq!(commit_status(&contradictory),("recovery_required",false,false));
    }
    #[test]fn partial_failed_or_duplicate_slot_rollback_never_claims_success() {
        let complete=report(false,vec!["trial failed".into()],(0..4).map(|slot|(slot,Ok(()))).collect(),None);
        assert_eq!(commit_status(&complete),("rolled_back",false,true));
        let partial=report(false,vec!["trial failed".into()],vec![(0,Ok(()))],None);
        assert_eq!(commit_status(&partial),("recovery_required",false,false));
        let failed=report(false,vec!["trial failed".into()],vec![(0,Ok(())),(1,Err("restore failed".into())),(2,Ok(())),(3,Ok(()))],None);
        assert_eq!(commit_status(&failed),("recovery_required",false,false));
        let duplicate=report(false,vec![],vec![(0,Ok(())),(0,Ok(())),(2,Ok(())),(3,Ok(()))],None);
        assert_eq!(commit_status(&duplicate),("recovery_required",false,false));
    }
    #[test]fn commit_json_retains_backup_report_and_journal_error() {
        let result=completed_json("123-456-1",json!({"before":[],"after":[]}),report(true,vec![],vec![],Some("journal unavailable".into())));
        assert_eq!(result["status"],"recovery_required");assert_eq!(result["success"],false);
        assert_eq!(result["backup_id"],"123-456-1");assert_eq!(result["plan_consumed"],true);
        assert_eq!(result["commit"]["report"]["journal_error"],"journal unavailable");
        assert_eq!(result["commit"]["report"]["transaction"]["applied"],true);
    }
    #[test]fn execute_error_is_not_success_and_preserves_one_use_backup_reference() {
        let result=execution_error_json("123-456-1",json!({}),io::Error::new(io::ErrorKind::PermissionDenied,"simulated native failure"));
        assert_eq!(result["status"],"recovery_required");assert_eq!(result["success"],false);
        assert_eq!(result["backup_id"],"123-456-1");assert_eq!(result["expires_seconds"],0);
        assert!(result["commit"]["report"].is_null());assert_eq!(result["commit"]["error"]["kind"],"permission_denied");
    }
}
