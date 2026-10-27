//! Safety-oriented account and placement primitives. This is not an archive engine.
#![forbid(unsafe_code)]

use std::collections::{BTreeMap, BTreeSet};

/// Identifies a provider service, not an individual login.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct ProviderKind(pub String);

/// A provider-issued identity. Email and display names must not be used as `subject`.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct ProviderIdentity {
    pub provider: ProviderKind,
    pub subject: String,
    pub tenant: Option<String>,
    pub region: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ConnectedAccount {
    /// A caller-generated, stable, globally unique internal identifier.
    pub id: String,
    pub identity: ProviderIdentity,
    pub label: String,
    /// Opaque reference into a secure credential store, never the credential itself.
    pub credential_ref: String,
}

#[derive(Debug, Eq, PartialEq)]
pub enum AccountError {
    InvalidIdentity,
    DuplicateAccountId,
    DuplicateIdentity { existing_account_id: String },
    AccountNotFound,
    IdentityMismatch,
}

/// In-memory only; callers must persist changes transactionally before use.
#[derive(Default)]
pub struct Accounts {
    by_id: BTreeMap<String, ConnectedAccount>,
    by_identity: BTreeMap<ProviderIdentity, String>,
}

impl Accounts {
    pub fn add(&mut self, account: ConnectedAccount) -> Result<(), AccountError> {
        if account.id.is_empty()
            || account.identity.provider.0.is_empty()
            || account.identity.subject.is_empty()
            || account.credential_ref.is_empty()
        {
            return Err(AccountError::InvalidIdentity);
        }
        if self.by_id.contains_key(&account.id) {
            return Err(AccountError::DuplicateAccountId);
        }
        if let Some(id) = self.by_identity.get(&account.identity) {
            return Err(AccountError::DuplicateIdentity {
                existing_account_id: id.clone(),
            });
        }
        self.by_identity
            .insert(account.identity.clone(), account.id.clone());
        self.by_id.insert(account.id.clone(), account);
        Ok(())
    }

    /// Reattach credentials only after the provider confirms the original identity.
    pub fn reconnect(
        &mut self,
        id: &str,
        identity: &ProviderIdentity,
        credential_ref: String,
    ) -> Result<(), AccountError> {
        if credential_ref.is_empty() {
            return Err(AccountError::InvalidIdentity);
        }
        let account = self
            .by_id
            .get_mut(id)
            .ok_or(AccountError::AccountNotFound)?;
        if &account.identity != identity {
            return Err(AccountError::IdentityMismatch);
        }
        account.credential_ref = credential_ref;
        Ok(())
    }

    pub fn get(&self, id: &str) -> Option<&ConnectedAccount> {
        self.by_id.get(id)
    }
}

/// The free space and reservations of one underlying quota, shared by all aliases.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct QuotaDomain {
    pub id: String,
    /// `None` means unknown, never unlimited. Values must be refreshed by the caller.
    pub free_bytes: Option<u64>,
    pub reserve_bytes: u64,
    pub in_flight_bytes: u64,
}

impl QuotaDomain {
    pub fn allocatable(&self) -> Option<u64> {
        self.free_bytes.map(|free| {
            free.saturating_sub(self.reserve_bytes)
                .saturating_sub(self.in_flight_bytes)
        })
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StorageResource {
    pub id: String,
    pub account_id: String,
    pub provider: ProviderKind,
    pub quota_domain_id: String,
    /// False for read-only, draining, or unavailable resources.
    pub accepts_writes: bool,
    /// Provider maximum object size, including encryption and encoding overhead.
    pub max_object_bytes: Option<u64>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Protection {
    OneCopy,
    SurviveProviderLoss,
}

#[derive(Debug, Eq, PartialEq)]
pub enum PlacementError {
    InvalidInput,
    DuplicateResource,
    DuplicateQuotaDomain,
    MissingQuotaDomain,
    SharedQuotaAcrossProviders,
    InsufficientKnownCapacity,
    InsufficientIndependentProviders,
}

/// Advisory selection only: it neither reserves capacity nor persists a job.
/// The caller must atomically reserve and revalidate before uploading.
pub fn plan_placement<'a>(
    resources: &'a [StorageResource],
    quotas: &[QuotaDomain],
    object_bytes: u64,
    protection: Protection,
) -> Result<Vec<&'a StorageResource>, PlacementError> {
    if object_bytes == 0 {
        return Err(PlacementError::InvalidInput);
    }
    let mut quota_map = BTreeMap::new();
    for quota in quotas {
        if quota.id.is_empty() {
            return Err(PlacementError::InvalidInput);
        }
        if quota_map.insert(&quota.id, quota).is_some() {
            return Err(PlacementError::DuplicateQuotaDomain);
        }
    }
    let mut ids = BTreeSet::new();
    let mut quota_providers = BTreeMap::new();
    let mut candidates = Vec::new();
    for resource in resources {
        if resource.id.is_empty()
            || resource.account_id.is_empty()
            || resource.provider.0.is_empty()
            || resource.quota_domain_id.is_empty()
        {
            return Err(PlacementError::InvalidInput);
        }
        if !ids.insert(&resource.id) {
            return Err(PlacementError::DuplicateResource);
        }
        let quota = quota_map
            .get(&resource.quota_domain_id)
            .ok_or(PlacementError::MissingQuotaDomain)?;
        if let Some(previous) =
            quota_providers.insert(&resource.quota_domain_id, &resource.provider)
            && previous != &resource.provider
        {
            return Err(PlacementError::SharedQuotaAcrossProviders);
        }
        if resource.accepts_writes
            && resource
                .max_object_bytes
                .is_none_or(|limit| object_bytes <= limit)
            && quota
                .allocatable()
                .is_some_and(|available| available >= object_bytes)
        {
            candidates.push((resource, quota.allocatable().unwrap_or(0)));
        }
    }
    // Deterministic selection, favouring the most free space. Alias resources never
    // add capacity: a provider-loss placement uses different providers and quotas.
    candidates.sort_by(|(a, av), (b, bv)| bv.cmp(av).then_with(|| a.id.cmp(&b.id)));
    let first = candidates
        .first()
        .ok_or(PlacementError::InsufficientKnownCapacity)?
        .0;
    if protection == Protection::OneCopy {
        return Ok(vec![first]);
    }
    let second = candidates
        .iter()
        .find(|(r, _)| r.provider != first.provider && r.quota_domain_id != first.quota_domain_id)
        .ok_or(PlacementError::InsufficientIndependentProviders)?
        .0;
    Ok(vec![first, second])
}
