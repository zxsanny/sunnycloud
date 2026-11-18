//! A fake `AssetSource` over in-memory assets. Tokens are opaque non-UTF-8 bytes holding a generation number, so a
//! round trip through the checkpoint proves the core never interprets them.

use std::collections::{BTreeMap, BTreeSet};
use std::io;
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};

use bytes::Bytes;
use mobile_backup::{
    AccessLevel, AssetDescriptor, AssetGone, AssetSource, ChangeToken, ReadableFd,
};
use sync_core::{FileIdentity, FileSource};

const TOKEN_PREFIX: [u8; 2] = [0xFF, 0xFE];

pub fn token_of(generation: u64) -> ChangeToken {
    let mut bytes = TOKEN_PREFIX.to_vec();
    bytes.extend_from_slice(&generation.to_be_bytes());
    ChangeToken(bytes)
}

fn generation_of(token: &ChangeToken) -> u64 {
    match token.0.as_slice() {
        [] => 0,
        [0xFF, 0xFE, rest @ ..] => u64::from_be_bytes(rest.try_into().expect("a fake token")),
        _ => panic!("a token the fake never issued"),
    }
}

/// Distinct content per asset, edit and size; the same arguments always give the same bytes.
pub fn content(asset_id: i64, edit: u32, size: usize) -> Vec<u8> {
    let seed = (asset_id as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ u64::from(edit);
    (0..size)
        .map(|index| (seed.wrapping_add(index as u64 * 31) >> ((index % 7) * 8)) as u8)
        .collect()
}

pub struct MemFile(Bytes);

impl FileSource for MemFile {
    fn len(&self) -> io::Result<u64> {
        Ok(self.0.len() as u64)
    }

    fn read_at(&self, offset: u64, len: u32) -> io::Result<Bytes> {
        let start = usize::try_from(offset).expect("offset fits");
        let end = (start + len as usize).min(self.0.len());
        Ok(self.0.slice(start..end))
    }

    fn identity(&self) -> io::Result<FileIdentity> {
        Ok(FileIdentity {
            size: self.0.len() as u64,
            mtime_ns: 0,
        })
    }
}

struct Item {
    generation: u64,
    edit: u32,
    size: usize,
    visible: bool,
}

#[derive(Default)]
struct State {
    items: BTreeMap<i64, Item>,
    generation: u64,
    limited: bool,
}

#[derive(Default)]
pub struct FakeSource {
    state: Mutex<State>,
    page_cap: Mutex<Option<u32>>,
    vanishing: Mutex<BTreeSet<i64>>,
    pub listed_pages: AtomicUsize,
    pub visible_id_pages: AtomicUsize,
    pub described: Mutex<Vec<i64>>,
}

impl FakeSource {
    fn state(&self) -> std::sync::MutexGuard<'_, State> {
        self.state.lock().expect("fake source state")
    }

    /// A host that answers at most `cap` assets per `assets_after`, whatever the limit asked.
    pub fn cap_pages(&self, cap: u32) {
        *self.page_cap.lock().expect("page cap") = Some(cap);
    }

    pub fn add(&self, asset_id: i64, size: usize) {
        self.insert(asset_id, size, true);
    }

    /// A pending or trashed row, or a photo the user has not granted: not visible yet.
    pub fn add_hidden(&self, asset_id: i64, size: usize) {
        self.insert(asset_id, size, false);
    }

    fn insert(&self, asset_id: i64, size: usize, visible: bool) {
        let mut state = self.state();
        state.generation += 1;
        let generation = state.generation;
        state.items.insert(
            asset_id,
            Item {
                generation,
                edit: 0,
                size,
                visible,
            },
        );
    }

    /// The row became final: it shows up with a new generation, as a MediaStore update does.
    pub fn finalize(&self, asset_id: i64) {
        let mut state = self.state();
        state.generation += 1;
        let generation = state.generation;
        let item = state.items.get_mut(&asset_id).expect("a known asset");
        item.visible = true;
        item.generation = generation;
    }

    /// A limited-access pick of an older photo: visible now, token unchanged.
    pub fn grant_keeping_token(&self, asset_id: i64) {
        self.state()
            .items
            .get_mut(&asset_id)
            .expect("a known asset")
            .visible = true;
    }

    pub fn edit(&self, asset_id: i64) {
        let mut state = self.state();
        state.generation += 1;
        let generation = state.generation;
        let item = state.items.get_mut(&asset_id).expect("a known asset");
        item.edit += 1;
        item.generation = generation;
    }

    /// Listed and described, but gone when the loop opens it.
    pub fn vanish_on_open(&self, asset_id: i64) {
        self.vanishing.lock().expect("vanishing").insert(asset_id);
    }

    pub fn set_limited(&self, limited: bool) {
        self.state().limited = limited;
    }

    pub fn bytes_of(&self, asset_id: i64) -> Vec<u8> {
        let state = self.state();
        let item = state.items.get(&asset_id).expect("a known asset");
        content(asset_id, item.edit, item.size)
    }

    fn descriptor(asset_id: i64, item: &Item) -> AssetDescriptor {
        AssetDescriptor {
            asset_id,
            change_token: token_of(item.generation),
            display_name: format!("IMG_{asset_id}.jpg"),
            mime: "image/jpeg".to_owned(),
            size: item.size as u64,
            date_taken: asset_id,
            relative_path: "DCIM/Camera".to_owned(),
        }
    }
}

impl AssetSource for FakeSource {
    fn assets_after(&self, token: &ChangeToken, limit: u32) -> Vec<AssetDescriptor> {
        self.listed_pages.fetch_add(1, Ordering::SeqCst);
        let after = generation_of(token);
        let limit = self
            .page_cap
            .lock()
            .expect("page cap")
            .map_or(limit, |cap| cap.min(limit));
        let state = self.state();
        let mut listed: Vec<(u64, i64)> = state
            .items
            .iter()
            .filter(|(_, item)| item.visible && item.generation > after)
            .map(|(id, item)| (item.generation, *id))
            .collect();
        listed.sort_unstable();
        listed
            .into_iter()
            .take(limit as usize)
            .map(|(_, id)| Self::descriptor(id, &state.items[&id]))
            .collect()
    }

    fn current_token(&self) -> ChangeToken {
        token_of(self.state().generation)
    }

    fn list_visible_ids(&self, page: u32) -> Vec<i64> {
        self.visible_id_pages.fetch_add(1, Ordering::SeqCst);
        self.state()
            .items
            .iter()
            .filter(|(_, item)| item.visible)
            .map(|(id, _)| *id)
            .skip(page as usize * 100)
            .take(100)
            .collect()
    }

    fn visible_count(&self) -> u64 {
        self.state()
            .items
            .values()
            .filter(|item| item.visible)
            .count() as u64
    }

    fn describe(&self, asset_ids: &[i64]) -> Vec<AssetDescriptor> {
        self.described.lock().expect("described").extend(asset_ids);
        let state = self.state();
        asset_ids
            .iter()
            .filter_map(|id| state.items.get(id).map(|item| Self::descriptor(*id, item)))
            .collect()
    }

    fn open_read(&self, asset_id: i64) -> Result<ReadableFd, AssetGone> {
        if self
            .vanishing
            .lock()
            .expect("vanishing")
            .contains(&asset_id)
        {
            return Err(AssetGone);
        }
        let state = self.state();
        let item = state.items.get(&asset_id).ok_or(AssetGone)?;
        Ok(std::sync::Arc::new(MemFile(Bytes::from(content(
            asset_id, item.edit, item.size,
        )))))
    }

    fn access_level(&self) -> AccessLevel {
        if self.state().limited {
            AccessLevel::Limited
        } else {
            AccessLevel::Full
        }
    }
}
