use crate::{snapshot::Snapshot, AtomString, InsertContent, Op, SmString, ID};
use rle::{HasLength, Mergable, Sliceable};
use std::alloc::Layout;

mod container_content;
pub use container_content::*;

pub trait Container {
    fn snapshot(&self) -> &dyn Snapshot;
    fn apply(&mut self, op: Op);
    fn type_id(&self) -> ContainerType;
}

#[derive(Hash, PartialEq, Eq, Debug, Clone)]
pub enum ContainerID {
    /// Root container does not need a insert op to create. It can be created implicitly.
    Root {
        name: AtomString,
        container_type: ContainerType,
    },
    Normal(ID),
}
