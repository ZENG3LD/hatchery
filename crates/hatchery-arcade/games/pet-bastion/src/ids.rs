//! Stable entity identifiers. Every tower, enemy and boss body gets one on
//! creation from a single monotonically increasing counter that never
//! reuses a value within a run -- this is the tie-breaker of last resort for
//! targeting ("route progress, then stable entity id").

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug, Hash)]
pub struct EntityId(pub u32);

#[derive(Clone, Debug, Default)]
pub struct EntityIdAllocator {
    next: u32,
}

impl EntityIdAllocator {
    pub fn next(&mut self) -> EntityId {
        let id = EntityId(self.next);
        self.next += 1;
        id
    }
}
