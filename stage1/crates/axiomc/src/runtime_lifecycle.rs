//! Target-neutral ownership and cleanup primitives for Runtime Lifecycle ABI v1.
//!
//! This module deliberately owns no backend layout or host handle.  It provides
//! the state machine that a backend can drive after lowering lifecycle MIR:
//! allocation failure is explicit, moves invalidate the source token, borrows
//! gate mutation and cleanup, and scope cleanup is deterministic.

use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
pub struct OwnerId(u64);

impl OwnerId {
    pub fn raw(self) -> u64 {
        self.0
    }
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
pub struct BorrowId(u64);

impl BorrowId {
    pub fn raw(self) -> u64 {
        self.0
    }
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
pub struct ResourceId(u64);

impl ResourceId {
    pub fn raw(self) -> u64 {
        self.0
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub enum ExitReason {
    NormalReturn,
    EarlyReturn,
    ErrorReturn,
    PanicUnwind,
    Cancellation,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum OwnerStatus {
    Active,
    Moved,
    Dropped,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ResourceStatus {
    Active,
    Moved,
    Closed,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BorrowMode {
    Shared,
    Mutable,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum LifecycleError {
    NoActiveScope,
    ScopeUnderflow,
    AllocationFailure { requested: usize, limit: usize },
    UnknownOwner(OwnerId),
    UseAfterMove(OwnerId),
    UseAfterDrop(OwnerId),
    DoubleFree(OwnerId),
    BorrowConflict(OwnerId),
    BorrowStillActive(OwnerId),
    UnknownBorrow(BorrowId),
    OwnershipEscape(OwnerId),
    NotCopyable(OwnerId),
    UnknownResource(ResourceId),
    ResourceUseAfterClose(ResourceId),
    ResourceEscape(ResourceId),
    DoubleClose(ResourceId),
}

impl LifecycleError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::NoActiveScope => "lifecycle.no_active_scope",
            Self::ScopeUnderflow => "lifecycle.scope_underflow",
            Self::AllocationFailure { .. } => "lifecycle.allocation_failure",
            Self::UnknownOwner(_) => "lifecycle.unknown_owner",
            Self::UseAfterMove(_) => "lifecycle.use_after_move",
            Self::UseAfterDrop(_) => "lifecycle.use_after_free",
            Self::DoubleFree(_) => "lifecycle.double_free",
            Self::BorrowConflict(_) | Self::BorrowStillActive(_) => "lifecycle.borrow_conflict",
            Self::UnknownBorrow(_) => "lifecycle.unknown_borrow",
            Self::OwnershipEscape(_) => "lifecycle.ownership_escape",
            Self::NotCopyable(_) => "lifecycle.copy_non_copyable",
            Self::UnknownResource(_) => "lifecycle.unknown_resource",
            Self::ResourceUseAfterClose(_) => "lifecycle.resource_use_after_close",
            Self::ResourceEscape(_) => "lifecycle.resource_escape",
            Self::DoubleClose(_) => "lifecycle.double_close",
        }
    }
}

impl fmt::Display for LifecycleError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::AllocationFailure { requested, limit } => {
                write!(
                    f,
                    "{}: requested {requested} bytes, limit is {limit}",
                    self.code()
                )
            }
            Self::UnknownOwner(id)
            | Self::UseAfterMove(id)
            | Self::UseAfterDrop(id)
            | Self::DoubleFree(id) => {
                write!(f, "{}: owner {}", self.code(), id.raw())
            }
            Self::BorrowConflict(id)
            | Self::BorrowStillActive(id)
            | Self::OwnershipEscape(id)
            | Self::NotCopyable(id) => write!(f, "{}: owner {}", self.code(), id.raw()),
            Self::UnknownBorrow(id) => write!(f, "{}: borrow {}", self.code(), id.raw()),
            Self::UnknownResource(id)
            | Self::ResourceUseAfterClose(id)
            | Self::ResourceEscape(id)
            | Self::DoubleClose(id) => write!(f, "{}: resource {}", self.code(), id.raw()),
            Self::NoActiveScope | Self::ScopeUnderflow => f.write_str(self.code()),
        }
    }
}

impl std::error::Error for LifecycleError {}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct LifecycleOperation {
    pub operation: String,
    pub allocation_effect: Option<String>,
    pub ownership_transfer: Option<String>,
    pub borrow_extent: Option<String>,
    pub cleanup_obligations: usize,
    pub resource_authority: Option<String>,
    pub source_provenance: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct AllocationInspection {
    pub owner: OwnerId,
    pub layout: usize,
    pub active: bool,
    pub copyable: bool,
    pub children: Vec<OwnerId>,
    pub outstanding_cleanup_obligation: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ResourceInspection {
    pub resource: ResourceId,
    pub capability: String,
    pub active: bool,
    pub outstanding_cleanup_obligation: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct LifecycleInspection {
    pub allocations: Vec<AllocationInspection>,
    pub resources: Vec<ResourceInspection>,
    pub operations: Vec<LifecycleOperation>,
}

#[derive(Clone, Debug)]
struct Allocation {
    bytes: Vec<u8>,
    copyable: bool,
}

#[derive(Clone, Debug)]
struct Owner {
    allocation: u64,
    status: OwnerStatus,
    parent: Option<OwnerId>,
    children: Vec<OwnerId>,
    borrows: BTreeSet<BorrowId>,
    source_provenance: Option<String>,
}

#[derive(Clone, Copy, Debug)]
struct Borrow {
    owner: OwnerId,
    mode: BorrowMode,
}

#[derive(Clone, Debug)]
struct Resource {
    status: ResourceStatus,
    capability: String,
    source_provenance: Option<String>,
}

#[derive(Clone, Debug)]
enum ScopeEntry {
    Owner(OwnerId),
    Resource(ResourceId),
}

#[derive(Clone, Debug, Default)]
struct Scope {
    entries: Vec<ScopeEntry>,
    deferred: Vec<String>,
}

/// A deterministic, target-neutral lifecycle state machine.
#[derive(Clone, Debug)]
pub struct LifecycleRuntime {
    next_id: u64,
    allocation_limit: usize,
    allocations: BTreeMap<u64, Allocation>,
    owners: BTreeMap<OwnerId, Owner>,
    borrows: BTreeMap<BorrowId, Borrow>,
    resources: BTreeMap<ResourceId, Resource>,
    scopes: Vec<Scope>,
    operations: Vec<LifecycleOperation>,
}

impl LifecycleRuntime {
    pub fn new(allocation_limit: usize) -> Self {
        Self {
            next_id: 1,
            allocation_limit,
            allocations: BTreeMap::new(),
            owners: BTreeMap::new(),
            borrows: BTreeMap::new(),
            resources: BTreeMap::new(),
            scopes: Vec::new(),
            operations: Vec::new(),
        }
    }

    pub fn enter_scope(&mut self) {
        self.scopes.push(Scope::default());
    }

    pub fn defer(&mut self, action: impl Into<String>) -> Result<(), LifecycleError> {
        let scope = self
            .scopes
            .last_mut()
            .ok_or(LifecycleError::NoActiveScope)?;
        scope.deferred.push(action.into());
        Ok(())
    }

    pub fn allocate(&mut self, layout: usize, copyable: bool) -> Result<OwnerId, LifecycleError> {
        self.allocate_with_source(layout, copyable, None)
    }

    pub fn allocate_with_source(
        &mut self,
        layout: usize,
        copyable: bool,
        source_provenance: Option<String>,
    ) -> Result<OwnerId, LifecycleError> {
        self.require_scope()?;
        self.ensure_capacity(layout)?;
        let allocation = self.next_id();
        let owner = OwnerId(self.next_id());
        self.allocations.insert(
            allocation,
            Allocation {
                bytes: vec![0; layout],
                copyable,
            },
        );
        self.owners.insert(
            owner,
            Owner {
                allocation,
                status: OwnerStatus::Active,
                parent: None,
                children: Vec::new(),
                borrows: BTreeSet::new(),
                source_provenance: source_provenance.clone(),
            },
        );
        self.scope_entry(ScopeEntry::Owner(owner))?;
        self.record(LifecycleOperation {
            operation: "allocate".into(),
            allocation_effect: Some(format!("allocate:{layout}")),
            ownership_transfer: Some(format!("creates:{}", owner.raw())),
            borrow_extent: None,
            cleanup_obligations: 1,
            resource_authority: None,
            source_provenance,
        });
        Ok(owner)
    }

    pub fn resize(&mut self, owner: OwnerId, layout: usize) -> Result<(), LifecycleError> {
        self.require_active_owner(owner)?;
        self.require_no_borrows(owner)?;
        self.ensure_capacity(layout)?;
        let allocation = self.owner(owner)?.allocation;
        self.allocations
            .get_mut(&allocation)
            .expect("active owner must have an allocation")
            .bytes
            .resize(layout, 0);
        self.record(LifecycleOperation {
            operation: "resize".into(),
            allocation_effect: Some(format!("resize:{layout}")),
            ownership_transfer: Some(format!("preserves:{}", owner.raw())),
            borrow_extent: None,
            cleanup_obligations: 1,
            resource_authority: None,
            source_provenance: self.owner(owner)?.source_provenance.clone(),
        });
        Ok(())
    }

    pub fn write(
        &mut self,
        owner: OwnerId,
        offset: usize,
        bytes: &[u8],
    ) -> Result<(), LifecycleError> {
        self.require_active_owner(owner)?;
        self.require_no_borrows(owner)?;
        let allocation = self.owner(owner)?.allocation;
        let storage = &mut self
            .allocations
            .get_mut(&allocation)
            .expect("active owner must have an allocation")
            .bytes;
        let end = offset.saturating_add(bytes.len());
        if end > storage.len() {
            return Err(LifecycleError::AllocationFailure {
                requested: end,
                limit: storage.len(),
            });
        }
        storage[offset..end].copy_from_slice(bytes);
        Ok(())
    }

    pub fn read(
        &self,
        owner: OwnerId,
        offset: usize,
        length: usize,
    ) -> Result<Vec<u8>, LifecycleError> {
        self.require_active_owner(owner)?;
        let allocation = self.owner(owner)?.allocation;
        let storage = &self
            .allocations
            .get(&allocation)
            .expect("active owner must have an allocation")
            .bytes;
        let end = offset.saturating_add(length);
        if end > storage.len() {
            return Err(LifecycleError::AllocationFailure {
                requested: end,
                limit: storage.len(),
            });
        }
        Ok(storage[offset..end].to_vec())
    }

    pub fn copy_value(&self, owner: OwnerId) -> Result<Vec<u8>, LifecycleError> {
        self.require_active_owner(owner)?;
        let allocation = self.owner(owner)?.allocation;
        let value = self
            .allocations
            .get(&allocation)
            .expect("active owner must have an allocation");
        if !value.copyable {
            return Err(LifecycleError::NotCopyable(owner));
        }
        Ok(value.bytes.clone())
    }

    pub fn clone_value(&mut self, owner: OwnerId) -> Result<OwnerId, LifecycleError> {
        self.require_active_owner(owner)?;
        self.require_no_borrows(owner)?;
        let source_allocation = self.owner(owner)?.allocation;
        let source = self
            .allocations
            .get(&source_allocation)
            .expect("active owner must have an allocation")
            .clone();
        let cloned = self.allocate_with_source(
            source.bytes.len(),
            source.copyable,
            self.owner(owner)?.source_provenance.clone(),
        )?;
        let cloned_allocation = self.owner(cloned)?.allocation;
        self.allocations
            .get_mut(&cloned_allocation)
            .expect("new owner must have an allocation")
            .bytes
            .copy_from_slice(&source.bytes);
        self.record(LifecycleOperation {
            operation: "clone".into(),
            allocation_effect: Some("clone".into()),
            ownership_transfer: Some(format!("distinct:{}", cloned.raw())),
            borrow_extent: None,
            cleanup_obligations: 2,
            resource_authority: None,
            source_provenance: self.owner(owner)?.source_provenance.clone(),
        });
        Ok(cloned)
    }

    pub fn move_value(&mut self, source: OwnerId) -> Result<OwnerId, LifecycleError> {
        self.require_active_owner(source)?;
        self.require_no_borrows(source)?;
        let previous = self.owner(source)?.clone();
        let moved = OwnerId(self.next_id());
        self.owners.insert(
            moved,
            Owner {
                allocation: previous.allocation,
                status: OwnerStatus::Active,
                parent: previous.parent,
                children: previous.children.clone(),
                borrows: BTreeSet::new(),
                source_provenance: previous.source_provenance.clone(),
            },
        );
        if let Some(parent) = previous.parent {
            let parent_owner = self.owner_mut(parent)?;
            parent_owner.children.retain(|child| *child != source);
            parent_owner.children.push(moved);
        }
        for child in &previous.children {
            self.owner_mut(*child)?.parent = Some(moved);
        }
        for scope in &mut self.scopes {
            for entry in &mut scope.entries {
                if matches!(entry, ScopeEntry::Owner(id) if *id == source) {
                    *entry = ScopeEntry::Owner(moved);
                }
            }
        }
        self.owner_mut(source)?.status = OwnerStatus::Moved;
        self.record(LifecycleOperation {
            operation: "move".into(),
            allocation_effect: None,
            ownership_transfer: Some(format!("{}->{}", source.raw(), moved.raw())),
            borrow_extent: None,
            cleanup_obligations: 1,
            resource_authority: None,
            source_provenance: previous.source_provenance,
        });
        Ok(moved)
    }

    pub fn attach_child(&mut self, parent: OwnerId, child: OwnerId) -> Result<(), LifecycleError> {
        self.require_active_owner(parent)?;
        self.require_active_owner(child)?;
        self.require_no_borrows(parent)?;
        self.require_no_borrows(child)?;
        if self.owner(child)?.parent.is_some() || parent == child {
            return Err(LifecycleError::OwnershipEscape(child));
        }
        self.remove_from_scopes(child);
        self.owner_mut(child)?.parent = Some(parent);
        self.owner_mut(parent)?.children.push(child);
        self.record(LifecycleOperation {
            operation: "attach".into(),
            allocation_effect: None,
            ownership_transfer: Some(format!("child:{}->parent:{}", child.raw(), parent.raw())),
            borrow_extent: None,
            cleanup_obligations: 2,
            resource_authority: None,
            source_provenance: self.owner(parent)?.source_provenance.clone(),
        });
        Ok(())
    }

    pub fn borrow(&mut self, owner: OwnerId, mode: BorrowMode) -> Result<BorrowId, LifecycleError> {
        self.require_active_owner(owner)?;
        let existing = self
            .owner(owner)?
            .borrows
            .iter()
            .filter_map(|id| self.borrows.get(id))
            .copied()
            .collect::<Vec<_>>();
        if mode == BorrowMode::Mutable
            || existing
                .iter()
                .any(|borrow| borrow.mode == BorrowMode::Mutable)
        {
            if !existing.is_empty() {
                return Err(LifecycleError::BorrowConflict(owner));
            }
        }
        let borrow = BorrowId(self.next_id());
        self.borrows.insert(borrow, Borrow { owner, mode });
        self.owner_mut(owner)?.borrows.insert(borrow);
        self.record(LifecycleOperation {
            operation: "borrow".into(),
            allocation_effect: None,
            ownership_transfer: Some(format!("borrows:{}", owner.raw())),
            borrow_extent: Some(format!("begin:{}", borrow.raw())),
            cleanup_obligations: 1,
            resource_authority: None,
            source_provenance: self.owner(owner)?.source_provenance.clone(),
        });
        Ok(borrow)
    }

    pub fn end_borrow(&mut self, borrow: BorrowId) -> Result<(), LifecycleError> {
        let record = self
            .borrows
            .remove(&borrow)
            .ok_or(LifecycleError::UnknownBorrow(borrow))?;
        self.owner_mut(record.owner)?.borrows.remove(&borrow);
        self.record(LifecycleOperation {
            operation: "borrow-end".into(),
            allocation_effect: None,
            ownership_transfer: None,
            borrow_extent: Some(format!("end:{}", borrow.raw())),
            cleanup_obligations: 1,
            resource_authority: None,
            source_provenance: self.owner(record.owner)?.source_provenance.clone(),
        });
        Ok(())
    }

    pub fn drop_value(&mut self, owner: OwnerId) -> Result<(), LifecycleError> {
        self.drop_owner(owner)
    }

    pub fn open_resource(
        &mut self,
        capability: impl Into<String>,
        source_provenance: Option<String>,
    ) -> Result<ResourceId, LifecycleError> {
        self.require_scope()?;
        let resource = ResourceId(self.next_id());
        self.resources.insert(
            resource,
            Resource {
                status: ResourceStatus::Active,
                capability: capability.into(),
                source_provenance: source_provenance.clone(),
            },
        );
        self.scope_entry(ScopeEntry::Resource(resource))?;
        self.record(LifecycleOperation {
            operation: "resource-open".into(),
            allocation_effect: None,
            ownership_transfer: Some(format!("creates:{}", resource.raw())),
            borrow_extent: None,
            cleanup_obligations: 1,
            resource_authority: self.resources.get(&resource).map(|r| r.capability.clone()),
            source_provenance,
        });
        Ok(resource)
    }

    pub fn move_resource(&mut self, source: ResourceId) -> Result<ResourceId, LifecycleError> {
        self.require_active_resource(source)?;
        let previous = self.resource(source)?.clone();
        let moved = ResourceId(self.next_id());
        self.resources.insert(
            moved,
            Resource {
                status: ResourceStatus::Active,
                capability: previous.capability.clone(),
                source_provenance: previous.source_provenance.clone(),
            },
        );
        for scope in &mut self.scopes {
            for entry in &mut scope.entries {
                if matches!(entry, ScopeEntry::Resource(id) if *id == source) {
                    *entry = ScopeEntry::Resource(moved);
                }
            }
        }
        self.resource_mut(source)?.status = ResourceStatus::Moved;
        self.record(LifecycleOperation {
            operation: "resource-move".into(),
            allocation_effect: None,
            ownership_transfer: Some(format!("{}->{}", source.raw(), moved.raw())),
            borrow_extent: None,
            cleanup_obligations: 1,
            resource_authority: Some(previous.capability),
            source_provenance: previous.source_provenance,
        });
        Ok(moved)
    }

    pub fn use_resource(&self, resource: ResourceId) -> Result<(), LifecycleError> {
        self.require_active_resource(resource)?;
        Ok(())
    }

    pub fn close_resource(&mut self, resource: ResourceId) -> Result<(), LifecycleError> {
        let status = self.resource(resource)?.status;
        match status {
            ResourceStatus::Active => {
                let value = self.resource(resource)?.clone();
                self.resource_mut(resource)?.status = ResourceStatus::Closed;
                self.record(LifecycleOperation {
                    operation: "resource-close".into(),
                    allocation_effect: None,
                    ownership_transfer: Some(format!("discharges:{}", resource.raw())),
                    borrow_extent: None,
                    cleanup_obligations: 0,
                    resource_authority: Some(value.capability),
                    source_provenance: value.source_provenance,
                });
                Ok(())
            }
            ResourceStatus::Closed => Err(LifecycleError::DoubleClose(resource)),
            ResourceStatus::Moved => Err(LifecycleError::ResourceUseAfterClose(resource)),
        }
    }

    pub fn drop_resource(&mut self, resource: ResourceId) -> Result<(), LifecycleError> {
        match self.resource(resource)?.status {
            ResourceStatus::Active => self.close_resource(resource),
            ResourceStatus::Closed => Ok(()),
            ResourceStatus::Moved => Err(LifecycleError::ResourceUseAfterClose(resource)),
        }
    }

    pub fn exit_scope(&mut self, reason: ExitReason) -> Result<(), LifecycleError> {
        let scope = self.scopes.pop().ok_or(LifecycleError::ScopeUnderflow)?;
        for action in scope.deferred.iter().rev() {
            self.record(LifecycleOperation {
                operation: "defer".into(),
                allocation_effect: None,
                ownership_transfer: None,
                borrow_extent: None,
                cleanup_obligations: 0,
                resource_authority: Some(action.clone()),
                source_provenance: None,
            });
        }
        let mut first_error = None;
        for entry in scope.entries.into_iter().rev() {
            let result = match entry {
                ScopeEntry::Owner(owner) => self.drop_owner(owner),
                ScopeEntry::Resource(resource) => self.drop_resource(resource),
            };
            if let Err(error) = result {
                first_error.get_or_insert(error);
            }
        }
        self.record(LifecycleOperation {
            operation: "scope-exit".into(),
            allocation_effect: None,
            ownership_transfer: Some(format!("{reason:?}")),
            borrow_extent: None,
            cleanup_obligations: self.outstanding_cleanup_obligations(),
            resource_authority: None,
            source_provenance: None,
        });
        first_error.map_or(Ok(()), Err)
    }

    pub fn outstanding_cleanup_obligations(&self) -> usize {
        self.owners
            .values()
            .filter(|owner| owner.status == OwnerStatus::Active)
            .count()
            + self
                .resources
                .values()
                .filter(|resource| resource.status == ResourceStatus::Active)
                .count()
    }

    pub fn inspect(&self) -> LifecycleInspection {
        let mut allocations = self
            .owners
            .iter()
            .map(|(owner_id, owner)| AllocationInspection {
                owner: *owner_id,
                layout: self
                    .allocations
                    .get(&owner.allocation)
                    .map_or(0, |allocation| allocation.bytes.len()),
                active: owner.status == OwnerStatus::Active,
                copyable: self
                    .allocations
                    .get(&owner.allocation)
                    .is_some_and(|allocation| allocation.copyable),
                children: owner.children.clone(),
                outstanding_cleanup_obligation: owner.status == OwnerStatus::Active,
            })
            .collect::<Vec<_>>();
        allocations.sort_by_key(|allocation| allocation.owner);
        let mut resources = self
            .resources
            .iter()
            .map(|(resource_id, resource)| ResourceInspection {
                resource: *resource_id,
                capability: resource.capability.clone(),
                active: resource.status == ResourceStatus::Active,
                outstanding_cleanup_obligation: resource.status == ResourceStatus::Active,
            })
            .collect::<Vec<_>>();
        resources.sort_by_key(|resource| resource.resource);
        LifecycleInspection {
            allocations,
            resources,
            operations: self.operations.clone(),
        }
    }

    fn drop_owner(&mut self, owner: OwnerId) -> Result<(), LifecycleError> {
        match self.owner(owner)?.status {
            OwnerStatus::Active => {}
            OwnerStatus::Moved => return Err(LifecycleError::UseAfterMove(owner)),
            OwnerStatus::Dropped => return Err(LifecycleError::DoubleFree(owner)),
        }
        self.require_no_borrows(owner)?;
        let children = self.owner(owner)?.children.clone();
        let allocation = self.owner(owner)?.allocation;
        self.owner_mut(owner)?.status = OwnerStatus::Dropped;
        self.owner_mut(owner)?.parent = None;
        for child in &children {
            self.owner_mut(*child)?.parent = None;
        }
        let mut first_error = None;
        for child in children.into_iter().rev() {
            if let Err(error) = self.drop_owner(child) {
                first_error.get_or_insert(error);
            }
        }
        self.allocations.remove(&allocation);
        self.remove_from_scopes(owner);
        self.record(LifecycleOperation {
            operation: "drop".into(),
            allocation_effect: Some("release".into()),
            ownership_transfer: Some(format!("discharges:{}", owner.raw())),
            borrow_extent: None,
            cleanup_obligations: self.outstanding_cleanup_obligations(),
            resource_authority: None,
            source_provenance: self.owner(owner)?.source_provenance.clone(),
        });
        first_error.map_or(Ok(()), Err)
    }

    fn ensure_capacity(&self, requested: usize) -> Result<(), LifecycleError> {
        if requested > self.allocation_limit {
            return Err(LifecycleError::AllocationFailure {
                requested,
                limit: self.allocation_limit,
            });
        }
        Ok(())
    }

    fn require_scope(&self) -> Result<(), LifecycleError> {
        if self.scopes.is_empty() {
            Err(LifecycleError::NoActiveScope)
        } else {
            Ok(())
        }
    }

    fn scope_entry(&mut self, entry: ScopeEntry) -> Result<(), LifecycleError> {
        self.scopes
            .last_mut()
            .ok_or(LifecycleError::NoActiveScope)?
            .entries
            .push(entry);
        Ok(())
    }

    fn remove_from_scopes(&mut self, owner: OwnerId) {
        for scope in &mut self.scopes {
            scope
                .entries
                .retain(|entry| !matches!(entry, ScopeEntry::Owner(id) if *id == owner));
        }
    }

    fn owner(&self, owner: OwnerId) -> Result<&Owner, LifecycleError> {
        self.owners
            .get(&owner)
            .ok_or(LifecycleError::UnknownOwner(owner))
    }

    fn owner_mut(&mut self, owner: OwnerId) -> Result<&mut Owner, LifecycleError> {
        self.owners
            .get_mut(&owner)
            .ok_or(LifecycleError::UnknownOwner(owner))
    }

    fn require_active_owner(&self, owner: OwnerId) -> Result<(), LifecycleError> {
        match self.owner(owner)?.status {
            OwnerStatus::Active => Ok(()),
            OwnerStatus::Moved => Err(LifecycleError::UseAfterMove(owner)),
            OwnerStatus::Dropped => Err(LifecycleError::UseAfterDrop(owner)),
        }
    }

    fn require_no_borrows(&self, owner: OwnerId) -> Result<(), LifecycleError> {
        if self.owner(owner)?.borrows.is_empty() {
            Ok(())
        } else {
            Err(LifecycleError::BorrowStillActive(owner))
        }
    }

    fn resource(&self, resource: ResourceId) -> Result<&Resource, LifecycleError> {
        self.resources
            .get(&resource)
            .ok_or(LifecycleError::UnknownResource(resource))
    }

    fn resource_mut(&mut self, resource: ResourceId) -> Result<&mut Resource, LifecycleError> {
        self.resources
            .get_mut(&resource)
            .ok_or(LifecycleError::UnknownResource(resource))
    }

    fn require_active_resource(&self, resource: ResourceId) -> Result<(), LifecycleError> {
        match self.resource(resource)?.status {
            ResourceStatus::Active => Ok(()),
            ResourceStatus::Moved | ResourceStatus::Closed => {
                Err(LifecycleError::ResourceUseAfterClose(resource))
            }
        }
    }

    fn next_id(&mut self) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        id
    }

    fn record(&mut self, operation: LifecycleOperation) {
        self.operations.push(operation);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resize_failure_preserves_original_allocation() {
        let mut runtime = LifecycleRuntime::new(8);
        runtime.enter_scope();
        let owner = runtime.allocate(4, false).unwrap();
        runtime.write(owner, 0, &[1, 2, 3, 4]).unwrap();
        let error = runtime.resize(owner, 9).unwrap_err();
        assert_eq!(error.code(), "lifecycle.allocation_failure");
        assert_eq!(runtime.read(owner, 0, 4).unwrap(), vec![1, 2, 3, 4]);
    }

    #[test]
    fn move_invalidates_source_and_transfers_cleanup() {
        let mut runtime = LifecycleRuntime::new(32);
        runtime.enter_scope();
        let source = runtime.allocate(3, false).unwrap();
        let moved = runtime.move_value(source).unwrap();
        assert_eq!(runtime.read(moved, 0, 3).unwrap(), vec![0, 0, 0]);
        assert_eq!(
            runtime.read(source, 0, 1).unwrap_err().code(),
            "lifecycle.use_after_move"
        );
        runtime.exit_scope(ExitReason::NormalReturn).unwrap();
        assert_eq!(runtime.outstanding_cleanup_obligations(), 0);
    }

    #[test]
    fn borrows_gate_move_write_and_drop_until_they_end() {
        let mut runtime = LifecycleRuntime::new(32);
        runtime.enter_scope();
        let owner = runtime.allocate(2, false).unwrap();
        let borrow = runtime.borrow(owner, BorrowMode::Mutable).unwrap();
        assert_eq!(
            runtime.move_value(owner).unwrap_err().code(),
            "lifecycle.borrow_conflict"
        );
        assert_eq!(
            runtime.write(owner, 0, &[7]).unwrap_err().code(),
            "lifecycle.borrow_conflict"
        );
        assert_eq!(
            runtime.drop_value(owner).unwrap_err().code(),
            "lifecycle.borrow_conflict"
        );
        runtime.end_borrow(borrow).unwrap();
        runtime.drop_value(owner).unwrap();
        assert_eq!(
            runtime.drop_value(owner).unwrap_err().code(),
            "lifecycle.double_free"
        );
    }

    #[test]
    fn nested_aggregate_drops_children_in_reverse_order() {
        let mut runtime = LifecycleRuntime::new(64);
        runtime.enter_scope();
        let parent = runtime.allocate(1, false).unwrap();
        let first = runtime.allocate(1, false).unwrap();
        let second = runtime.allocate(1, false).unwrap();
        runtime.attach_child(parent, first).unwrap();
        runtime.attach_child(parent, second).unwrap();
        runtime.drop_value(parent).unwrap();
        let drops = runtime
            .inspect()
            .operations
            .iter()
            .filter(|operation| operation.operation == "drop")
            .map(|operation| operation.ownership_transfer.clone().unwrap())
            .collect::<Vec<_>>();
        assert_eq!(drops, vec!["discharges:6", "discharges:4", "discharges:2"]);
    }

    #[test]
    fn scope_exit_runs_defers_before_reverse_cleanup_and_closes_resources() {
        let mut runtime = LifecycleRuntime::new(32);
        runtime.enter_scope();
        runtime.defer("outer").unwrap();
        let owner = runtime.allocate(1, false).unwrap();
        let resource = runtime
            .open_resource("fs:read", Some("main.ax:4".into()))
            .unwrap();
        runtime.defer("inner").unwrap();
        runtime.exit_scope(ExitReason::PanicUnwind).unwrap();
        assert_eq!(
            runtime.use_resource(resource).unwrap_err().code(),
            "lifecycle.resource_use_after_close"
        );
        let operations = runtime.inspect().operations;
        let defer_names = operations
            .iter()
            .filter(|operation| operation.operation == "defer")
            .map(|operation| operation.resource_authority.as_deref().unwrap())
            .collect::<Vec<_>>();
        assert_eq!(defer_names, vec!["inner", "outer"]);
        assert!(operations.iter().any(|operation| {
            operation.operation == "drop"
                && operation.ownership_transfer.as_deref()
                    == Some(&format!("discharges:{}", owner.raw()))
        }));
    }

    #[test]
    fn resource_close_is_single_discharge_and_move_preserves_authority() {
        let mut runtime = LifecycleRuntime::new(16);
        runtime.enter_scope();
        let source = runtime.open_resource("net:connect", None).unwrap();
        let moved = runtime.move_resource(source).unwrap();
        assert_eq!(
            runtime.use_resource(source).unwrap_err().code(),
            "lifecycle.resource_use_after_close"
        );
        runtime.close_resource(moved).unwrap();
        assert_eq!(
            runtime.close_resource(moved).unwrap_err().code(),
            "lifecycle.double_close"
        );
    }
}
