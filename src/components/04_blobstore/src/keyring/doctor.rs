//! The master key check of `sunnycloud doctor`: the same comparison as the start, without writing anything.

use contracts::{
    DoctorCheck, DoctorContext, DoctorTag, Finding, FindingLevel, KeyError, async_trait,
};
use sc_db::Db;
use sc_observability::log_error;

use super::ring::KeyRing;
use super::{compare_with_ring, keyring_repo, volume, volume_root};
use crate::config::BlobstoreConfig;
use crate::error::operator_message;

const REMEDY: &str = "mount the master key of this installation; `sunnycloud admin keycheck-init` records \
     it only after the key is confirmed";

pub struct KeyDoctorCheck {
    config: BlobstoreConfig,
    db: Db,
}

impl KeyDoctorCheck {
    pub fn new(config: BlobstoreConfig, db: Db) -> Self {
        KeyDoctorCheck { config, db }
    }

    /// A pre-flight run opens no database connection, so a missing key check is then not judged.
    async fn inspect(&self, pre_flight: bool) -> Result<&'static str, KeyError> {
        let ring = KeyRing::load(&self.config).await?;
        let root = volume_root(&self.config).await?;
        match volume::read_key_check(&root).await? {
            Some(check) => {
                compare_with_ring(&check, &ring)?;
                Ok("master key matches the key check")
            }
            None if pre_flight => Ok("no key check file yet; master key length is valid"),
            None => {
                if volume::has_blob_files(&root).await? || keyring_repo::any_blob(&self.db).await? {
                    Err(KeyError::KeyCheckAbsentButBlobsExist)
                } else {
                    Ok("no key check file yet; it is created at the first start")
                }
            }
        }
    }
}

#[async_trait]
impl DoctorCheck for KeyDoctorCheck {
    fn name(&self) -> &'static str {
        "master_key"
    }

    fn tags(&self) -> &[DoctorTag] {
        &[DoctorTag::PreFlight, DoctorTag::Restore]
    }

    /// Every refusal that stops a start is a `Fail`; the message is the operator text, which holds no key bytes.
    async fn run(&self, ctx: &DoctorContext) -> Vec<Finding> {
        match self.inspect(ctx.pre_flight).await {
            Ok(message) => vec![Finding {
                level: FindingLevel::Ok,
                message: message.to_owned(),
                remedy: String::new(),
            }],
            Err(error) => {
                log_error!("master key check failed", err = &error);
                vec![Finding {
                    level: FindingLevel::Fail,
                    message: operator_message(&error),
                    remedy: REMEDY.to_owned(),
                }]
            }
        }
    }
}
