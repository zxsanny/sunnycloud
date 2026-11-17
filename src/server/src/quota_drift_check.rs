//! The quota drift report of `doctor` (E-22/c14b, AC-74): for every account, the bytes 02 counts for its live
//! files and the bytes 03 holds for its open uploads go to `QuotaAdmin::recompute(.., repair = false)`, which
//! compares them with the stored usage and changes nothing. Only the composition root sees all three owners.

use std::sync::Arc;

use contracts::{
    AccountAdmin, AccountFilter, AccountStatus, DoctorCheck, DoctorContext, DoctorTag, Finding,
    FindingLevel, LiveUsageSource, Page, QuotaAdmin, ReservationSource, async_trait,
};
use sc_observability::log_error;
use uuid::Uuid;

const PAGE: u32 = 100;

pub struct QuotaDriftCheck {
    accounts: Arc<dyn AccountAdmin>,
    live: Arc<dyn LiveUsageSource>,
    reserved: Arc<dyn ReservationSource>,
    quota: Arc<dyn QuotaAdmin>,
}

impl QuotaDriftCheck {
    pub fn new(
        accounts: Arc<dyn AccountAdmin>,
        live: Arc<dyn LiveUsageSource>,
        reserved: Arc<dyn ReservationSource>,
        quota: Arc<dyn QuotaAdmin>,
    ) -> Self {
        QuotaDriftCheck {
            accounts,
            live,
            reserved,
            quota,
        }
    }

    /// `None` when the stored usage matches; a finding when it drifted or could not be compared.
    async fn compare(&self, account: Uuid) -> Option<Finding> {
        match self.drift(account).await {
            Ok((stored, computed)) if stored == computed => None,
            Ok((stored, computed)) => Some(warn(
                format!(
                    "account {account}: stored usage {stored} bytes, computed {computed} bytes"
                ),
                "the usage row is out of step with the files; no command repairs it yet, so report the account",
            )),
            Err(step) => Some(warn(
                format!("account {account}: the usage could not be compared ({step})"),
                "run doctor again once the database answers",
            )),
        }
    }

    async fn drift(&self, account: Uuid) -> Result<(i64, i64), &'static str> {
        let live = self.live.live_bytes(account).await.map_err(|error| {
            log_error!("live usage read failed", account_id = account, err = &error);
            "live bytes"
        })?;
        let reserved = self
            .reserved
            .open_reserved_bytes(account)
            .await
            .map_err(|error| {
                log_error!(
                    "reserved bytes read failed",
                    account_id = account,
                    err = &error
                );
                "reserved bytes"
            })?;
        let drift = self
            .quota
            .recompute(account, live, reserved, false)
            .await
            .map_err(|error| {
                log_error!("quota recompute failed", account_id = account, err = &error);
                "recompute"
            })?;
        Ok((drift.stored, drift.computed))
    }
}

fn warn(message: String, remedy: &str) -> Finding {
    Finding {
        level: FindingLevel::Warn,
        message,
        remedy: remedy.to_owned(),
    }
}

#[async_trait]
impl DoctorCheck for QuotaDriftCheck {
    fn name(&self) -> &'static str {
        "quota_drift"
    }

    fn tags(&self) -> &[DoctorTag] {
        &[DoctorTag::Restore]
    }

    /// An account being deleted is skipped: its usage goes with it.
    async fn run(&self, _ctx: &DoctorContext) -> Vec<Finding> {
        let mut findings = Vec::new();
        let mut offset = 0u64;
        loop {
            let page = Page {
                offset,
                limit: PAGE,
            };
            let rows = match self
                .accounts
                .list_accounts(AccountFilter::default(), page)
                .await
            {
                Ok(rows) => rows,
                Err(error) => {
                    log_error!("account list failed", err = &error);
                    findings.push(warn(
                        "the accounts could not be listed, so quota drift is not checked"
                            .to_owned(),
                        "run doctor again once the database answers",
                    ));
                    return findings;
                }
            };
            if rows.is_empty() {
                break;
            }
            offset += rows.len() as u64;
            for account in rows
                .iter()
                .filter(|row| row.state != AccountStatus::Deleting)
            {
                findings.extend(self.compare(account.id).await);
            }
        }
        if findings.is_empty() {
            findings.push(Finding {
                level: FindingLevel::Ok,
                message: "stored usage matches the files for every account".to_owned(),
                remedy: String::new(),
            });
        }
        findings
    }
}
