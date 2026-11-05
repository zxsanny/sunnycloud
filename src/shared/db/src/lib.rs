/// Handle to an open database transaction.
///
/// It has no public constructor: only this crate can create one, so a port that takes
/// `&mut DbTx` cannot be called with a value built elsewhere.
///
/// ```compile_fail,E0423
/// let _tx = sc_db::DbTx;
/// ```
#[non_exhaustive]
pub struct DbTx;
