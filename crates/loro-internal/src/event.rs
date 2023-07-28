use enum_as_inner::EnumAsInner;
use serde::{Deserialize, Serialize};
use smallvec::SmallVec;

use crate::{
    container::ContainerID,
    delta::{Delta, DeltaType, MapDelta, MapDiff, Meta},
    text::text_content::SliceRanges,
    transaction::Origin,
    version::Frontiers,
    InternalString, LoroValue,
};

#[derive(Debug)]
pub(crate) struct EventDiff {
    pub id: ContainerID,
    pub diff: SmallVec<[Diff; 1]>,
    pub local: bool,
}

#[derive(Debug)]
pub(crate) struct RawEvent {
    pub container_id: ContainerID,
    pub old_version: Frontiers,
    pub new_version: Frontiers,
    pub local: bool,
    pub diff: Diff,
    pub abs_path: Path,
    pub origin: Option<Origin>,
}

#[derive(Debug, Serialize, Clone)]
pub struct Event {
    pub old_version: Frontiers,
    pub new_version: Frontiers,
    pub current_target: Option<ContainerID>,
    pub target: ContainerID,
    /// the relative path from current_target to target
    pub relative_path: Path,
    pub absolute_path: Path,
    pub diff: Diff,
    pub local: bool,
    pub origin: Option<Origin>,
}

#[derive(Debug)]
pub(crate) struct PathAndTarget {
    pub relative_path: Path,
    pub target: Option<ContainerID>,
}

#[derive(Debug, Default)]
pub(crate) struct EventDispatch {
    pub sub_ids: Vec<SubscriptionID>,
    pub rewrite: Option<PathAndTarget>,
}

pub type Path = SmallVec<[Index; 4]>;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Index {
    Key(InternalString),
    Seq(usize),
}

#[repr(transparent)]
#[derive(Default, Clone, Copy, Debug, Serialize, PartialEq)]
pub struct Utf16Meta {
    pub utf16_len: Option<usize>,
}

impl Meta for Utf16Meta {
    fn empty() -> Self {
        Utf16Meta { utf16_len: None }
    }

    fn is_empty(&self) -> bool {
        self.utf16_len.is_none()
    }

    fn compose(&mut self, _: &Self, _: (DeltaType, DeltaType)) {}

    fn take(&mut self, other: &Self) -> Self {
        if let Some(utf16_len) = &mut self.utf16_len {
            let other_len = other.utf16_len.unwrap_or(0);
            debug_assert!(
                other_len <= *utf16_len,
                "other_len: {}, utf16_len: {}",
                other_len,
                utf16_len
            );
            *utf16_len -= other_len;
        }

        *other
    }

    fn is_mergeable(&self, _: &Self) -> bool {
        true
    }

    fn merge(&mut self, other: &Self) {
        match (&mut self.utf16_len, &other.utf16_len) {
            (Some(a), Some(b)) => {
                *a += *b;
            }
            (a, _) => {
                *a = None;
            }
        }
    }
}

impl Utf16Meta {
    pub(crate) fn new(utf16_len: usize) -> Self {
        Utf16Meta {
            utf16_len: Some(utf16_len),
        }
    }
}

/// Diff is the diff between two versions of a container.
/// It's used to describe the change of a container and the events.
///
/// # Internal
///
/// SeqRaw & SeqRawUtf16 is internal stuff, it should not be exposed to user.
/// The len inside SeqRaw uses utf8 for Text by default.
///
/// Text always uses platform specific indexes:
///
/// - When `wasm` is enabled, it should use utf16 indexes.
/// - When `wasm` is disabled, it should use utf8 indexes.
#[derive(Clone, Debug, EnumAsInner, Serialize)]
pub enum Diff {
    List(Delta<Vec<LoroValue>>),
    SeqRaw(Delta<SliceRanges>),
    SeqRawUtf16(Delta<SliceRanges>),
    Text(Delta<String, Utf16Meta>),
    /// @deprecated
    Map(MapDiff<LoroValue>),
    NewMap(MapDelta),
}

impl Diff {
    pub(crate) fn compose(self, diff: Diff) -> Result<Diff, Self> {
        // PERF: avoid clone
        match (self, diff) {
            (Diff::List(a), Diff::List(b)) => Ok(Diff::List(a.compose(b))),
            (Diff::SeqRaw(a), Diff::SeqRaw(b)) => Ok(Diff::SeqRaw(a.compose(b))),
            (Diff::Text(a), Diff::Text(b)) => Ok(Diff::Text(a.compose(b))),
            (Diff::Map(a), Diff::Map(b)) => Ok(Diff::Map(a.compose(b))),
            (Diff::NewMap(a), Diff::NewMap(b)) => Ok(Diff::NewMap(a.compose(b))),
            (a, _) => Err(a),
        }
    }
}

impl Default for Diff {
    fn default() -> Self {
        Diff::List(Delta::default())
    }
}

// pub type Observer = Box<dyn FnMut(&Event) + Send>;
#[derive(Default)]
pub(crate) struct ObserverOptions {
    pub(crate) once: bool,
    pub(crate) container: Option<ContainerID>,
    pub(crate) deep: bool,
}

impl ObserverOptions {
    fn with_container(mut self, container: ContainerID) -> Self {
        self.container.replace(container);
        self
    }
}

pub type ObserverHandler = Box<dyn FnMut(&Event) + Send>;

pub(crate) struct Observer {
    handler: ObserverHandler,
    options: ObserverOptions,
}

impl Observer {
    pub fn new_root(handler: ObserverHandler) -> Self {
        Self {
            handler,
            options: ObserverOptions::default(),
        }
    }

    pub fn new_container(handler: ObserverHandler, container: ContainerID) -> Self {
        Self {
            handler,
            options: ObserverOptions::default().with_container(container),
        }
    }

    pub fn container(&self) -> &Option<ContainerID> {
        &self.options.container
    }

    pub fn root(&self) -> bool {
        self.options.container.is_none()
    }

    pub fn deep(&self) -> bool {
        self.options.deep
    }

    pub fn with_once(mut self, once: bool) -> Self {
        self.options.once = once;
        self
    }

    pub fn with_deep(mut self, deep: bool) -> Self {
        self.options.deep = deep;
        self
    }

    pub fn once(&self) -> bool {
        self.options.once
    }

    pub fn call(&mut self, event: &Event) {
        (self.handler)(event)
    }
}

pub type SubscriptionID = u32;
