//! Finding assets: new ones after the change token, and the ones a token cannot reveal (older photos that became
//! visible under limited access) by the visible-id diff. Rows are stored before the token moves, so a kill after
//! listing loses no asset.

use tracing::warn;

use super::Backup;
use crate::outcome::{RunContext, RunError, Trigger};
use crate::source::{AccessLevel, AssetSource};

const PAGE: u32 = 500;
const TOKEN_KEY: &str = "change_token";
const LEVEL_KEY: &str = "access_level";
const COUNT_KEY: &str = "visible_count";

impl Backup {
    pub(super) fn discover(
        &self,
        ctx: &RunContext,
        source: &dyn AssetSource,
        level: AccessLevel,
    ) -> Result<(), RunError> {
        self.list_new(source)?;
        let seen = (
            self.assets.cursor_text(LEVEL_KEY)?,
            self.assets.cursor_text(COUNT_KEY)?,
        );
        let now = (
            level.as_text().to_owned(),
            source.visible_count().to_string(),
        );
        if ctx.trigger == Trigger::CatchUp || seen != (Some(now.0.clone()), Some(now.1.clone())) {
            self.visible_diff(source)?;
            self.assets.set_cursor_text(LEVEL_KEY, &now.0)?;
            self.assets.set_cursor_text(COUNT_KEY, &now.1)?;
        }
        Ok(())
    }

    /// The target token is read before the listing, so an asset added meanwhile is listed again next run.
    fn list_new(&self, source: &dyn AssetSource) -> Result<(), RunError> {
        let target = source.current_token();
        let stored = self.assets.cursor_token(TOKEN_KEY)?.unwrap_or_default();
        let mut after = stored.clone();
        loop {
            let page = source.assets_after(&after, PAGE);
            let Some(last) = page.last() else { break };
            self.assets.store_listed(&page)?;
            if last.change_token == after {
                warn!("the asset source did not advance its change token, listing stops");
                break;
            }
            after = last.change_token.clone();
        }
        if target != stored {
            self.assets.set_cursor_token(TOKEN_KEY, &target)?;
        }
        Ok(())
    }

    fn visible_diff(&self, source: &dyn AssetSource) -> Result<(), RunError> {
        let mut page = 0;
        loop {
            let ids = source.list_visible_ids(page);
            if ids.is_empty() {
                return Ok(());
            }
            let unknown = self.assets.unknown_ids(&ids)?;
            if !unknown.is_empty() {
                self.assets.store_unknown(&source.describe(&unknown))?;
            }
            page += 1;
        }
    }
}
