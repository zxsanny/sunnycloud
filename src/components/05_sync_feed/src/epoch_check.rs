use contracts::{Finding, FindingLevel, Marker, MarkerStatus};

const REMEDY: &str = "run `sunnycloud admin epoch-bump`";

/// The findings of the epoch doctor check for the database epoch against the `.epoch` marker.
/// `holds_blobs` is whether the blob volume holds blobs: an absent marker is only a problem then.
pub fn epoch_findings(status: MarkerStatus, holds_blobs: bool) -> Vec<Finding> {
    let MarkerStatus { db_epoch, marker } = status;
    let finding = match marker {
        Marker::Absent if holds_blobs => {
            fail("the .epoch marker is absent on a volume that holds blobs".to_owned())
        }
        Marker::Absent => {
            ok("no .epoch marker yet; it is written with the first account".to_owned())
        }
        Marker::Present(marker) if marker == db_epoch => ok(format!(
            "database epoch {db_epoch} matches the .epoch marker"
        )),
        Marker::Present(marker) => fail(format!(
            "database epoch {db_epoch} differs from the .epoch marker {marker}"
        )),
    };
    vec![finding]
}

fn ok(message: String) -> Finding {
    Finding {
        level: FindingLevel::Ok,
        message,
        remedy: String::new(),
    }
}

fn fail(message: String) -> Finding {
    Finding {
        level: FindingLevel::Fail,
        message,
        remedy: REMEDY.to_owned(),
    }
}
