use cloudweave_domain::*;

fn identity(subject: &str) -> ProviderIdentity {
    ProviderIdentity {
        provider: ProviderKind("google".into()),
        subject: subject.into(),
        tenant: None,
        region: None,
    }
}
fn account(id: &str, subject: &str) -> ConnectedAccount {
    ConnectedAccount {
        id: id.into(),
        identity: identity(subject),
        label: "Personal".into(),
        credential_ref: format!("secret:{id}"),
    }
}
fn resource(id: &str, provider: &str, quota: &str) -> StorageResource {
    StorageResource {
        id: id.into(),
        account_id: id.into(),
        provider: ProviderKind(provider.into()),
        quota_domain_id: quota.into(),
        accepts_writes: true,
        max_object_bytes: None,
    }
}
fn quota(id: &str, free: Option<u64>) -> QuotaDomain {
    QuotaDomain {
        id: id.into(),
        free_bytes: free,
        reserve_bytes: 10,
        in_flight_bytes: 5,
    }
}

#[test]
fn same_provider_accounts_are_isolated_and_duplicate_identity_is_rejected() {
    let mut accounts = Accounts::default();
    for (id, subject) in [("a", "1"), ("b", "2"), ("c", "3")] {
        accounts.add(account(id, subject)).unwrap();
    }
    assert_eq!(
        accounts.add(account("d", "2")),
        Err(AccountError::DuplicateIdentity {
            existing_account_id: "b".into()
        })
    );
    assert_eq!(accounts.get("a").unwrap().credential_ref, "secret:a");
    assert_eq!(accounts.get("b").unwrap().credential_ref, "secret:b");
    assert!(accounts.get("d").is_none());
}

#[test]
fn reconnect_cannot_silently_replace_identity_or_credential() {
    let mut accounts = Accounts::default();
    accounts.add(account("a", "1")).unwrap();
    assert_eq!(
        accounts.reconnect("a", &identity("2"), "wrong".into()),
        Err(AccountError::IdentityMismatch)
    );
    assert_eq!(accounts.get("a").unwrap().credential_ref, "secret:a");
    accounts
        .reconnect("a", &identity("1"), "new".into())
        .unwrap();
    assert_eq!(accounts.get("a").unwrap().credential_ref, "new");
}

#[test]
fn two_google_accounts_do_not_protect_against_google_outage() {
    let resources = [resource("a", "google", "a"), resource("b", "google", "b")];
    let quotas = [quota("a", Some(100)), quota("b", Some(100))];
    assert_eq!(
        plan_placement(&resources, &quotas, 50, Protection::SurviveProviderLoss),
        Err(PlacementError::InsufficientIndependentProviders)
    );
}

#[test]
fn selects_independent_providers_with_reserves_and_limits() {
    let mut resources = [
        resource("a", "google", "a"),
        resource("b", "google", "b"),
        resource("c", "dropbox", "c"),
    ];
    resources[1].max_object_bytes = Some(20);
    let quotas = [
        quota("a", Some(100)),
        quota("b", Some(1000)),
        quota("c", Some(80)),
    ];
    let selected =
        plan_placement(&resources, &quotas, 50, Protection::SurviveProviderLoss).unwrap();
    assert_eq!(
        selected.iter().map(|r| r.id.as_str()).collect::<Vec<_>>(),
        vec!["a", "c"]
    );
    assert_eq!(
        plan_placement(&resources, &quotas, 70, Protection::SurviveProviderLoss),
        Err(PlacementError::InsufficientIndependentProviders)
    );
}

#[test]
fn unknown_or_exhausted_quota_and_draining_resources_are_never_selected() {
    let mut resources = [
        resource("a", "google", "a"),
        resource("b", "dropbox", "b"),
        resource("c", "box", "c"),
    ];
    resources[2].accepts_writes = false;
    let quotas = [
        quota("a", None),
        quota("b", Some(10)),
        quota("c", Some(1000)),
    ];
    assert_eq!(
        plan_placement(&resources, &quotas, 1, Protection::OneCopy),
        Err(PlacementError::InsufficientKnownCapacity)
    );
}

#[test]
fn inconsistent_or_duplicate_domains_are_rejected() {
    let resources = [
        resource("a", "google", "shared"),
        resource("b", "dropbox", "shared"),
    ];
    assert_eq!(
        plan_placement(
            &resources,
            &[quota("shared", Some(100))],
            1,
            Protection::SurviveProviderLoss
        ),
        Err(PlacementError::SharedQuotaAcrossProviders)
    );
    assert_eq!(
        plan_placement(
            &resources,
            &[quota("shared", Some(100)), quota("shared", Some(100))],
            1,
            Protection::OneCopy
        ),
        Err(PlacementError::DuplicateQuotaDomain)
    );
}
