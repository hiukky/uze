//! Capability conformance (L1): `package_exposure_plan` exact-coverage
//! semantics — generated native projection, explicit envelope precedence,
//! unsafe-declaration rejection and fallback.
//!
//! **Every question here is asked of every registered harness.** The subject
//! list comes from `IntegrationRegistry::isolated` (see `super::subjects`),
//! so a harness added to the product enters this suite automatically and
//! must either answer each question or declare, with a reason, that it has
//! no surface for it. That replaces the hand-copied per-vendor blocks this
//! file used to carry: three copies of the same scenario could not tell a
//! deliberate exemption from a forgotten one, and the fifth harness would
//! have started with none of them.
//!
//! Every assertion is phrased as an *outcome* invariant — route, coverage
//! set, fallback survival — never as "the JSON must look like X". A vendor's
//! manifest shape is knowledge held in exactly one place, `subjects.rs`.

use std::{collections::BTreeSet, fs};

use uze_core::{
    exposure::ExposureMechanism, integration::IntegrationPort, project::Resource,
    router::CompatibilityRoute, store::StoredPackage,
};

use super::{
    fixtures::{
        build_package, mark_setup, mcp_resource, skill_resource,
        skill_resource_outside_conventions, tree,
    },
    subjects::{Subject, subjects},
};

// ============================================================================
// The shared assertions. Each states one invariant, against `&dyn
// IntegrationPort` and nothing more.
// ============================================================================

fn assert_exact_package_coverage(
    integration: &dyn IntegrationPort,
    package: &StoredPackage,
    resources: &[&Resource],
    expected_covered: &BTreeSet<String>,
    case: &str,
) {
    let plan = integration
        .package_exposure_plan(package, resources)
        .unwrap_or_else(|| panic!("[{case}] expected a package exposure plan"));
    assert_eq!(
        plan.route,
        CompatibilityRoute::Native,
        "[{case}] a package delivery plan must route Native"
    );
    assert_eq!(
        &plan.provided_resource_identities, expected_covered,
        "[{case}] provided_resource_identities must be an exact discovered ∩ declared intersection"
    );
    assert_every_uncovered_resource_still_falls_back(
        integration,
        resources,
        expected_covered,
        case,
    );
}

/// An uncovered resource must still resolve through the normal per-resource
/// fallback, never silently disappear.
fn assert_every_uncovered_resource_still_falls_back(
    integration: &dyn IntegrationPort,
    resources: &[&Resource],
    covered: &BTreeSet<String>,
    case: &str,
) {
    for resource in resources {
        if !covered.contains(&resource.identity()) {
            assert_fallback_survives(integration, resource, case);
        }
    }
}

fn assert_fallback_survives(integration: &dyn IntegrationPort, resource: &Resource, case: &str) {
    let fallback = integration.exposure_plan(resource);
    assert!(
        !matches!(fallback.mechanism, ExposureMechanism::Unsupported { .. }),
        "[{case}] resource {} must still route through capability-level fallback, not Unsupported",
        resource.identity()
    );
}

/// Presence of an explicit envelope — not its validity — decides the branch.
/// A malformed envelope must never be papered over by generation, and the two
/// legal shapes of that outcome are both stated here: a plan on the explicit
/// route declaring nothing, or no native plan at all where the envelope *is*
/// the vendor manifest.
fn assert_a_malformed_envelope_never_becomes_coverage(
    integration: &dyn IntegrationPort,
    package: &StoredPackage,
    resource: &Resource,
    case: &str,
) {
    // Two legal shapes, both stated: a plan on the explicit route declaring
    // nothing, or no native plan at all where the envelope *is* the vendor
    // manifest. Either way generation never papered over it.
    if let Some(plan) = integration.package_exposure_plan(package, &[resource]) {
        assert!(
            plan.provided_resource_identities.is_empty(),
            "[{case}] a malformed declaration must yield empty coverage — never a crash, and \
             never the full coverage generation would have produced"
        );
    }
    assert_fallback_survives(integration, resource, case);
}

/// An invalid or unsafe declaration can never be made valid by destructive
/// normalization, and must not suppress the resource's own delivery.
fn assert_an_unsafe_declaration_never_covers_the_colliding_resource(
    integration: &dyn IntegrationPort,
    package: &StoredPackage,
    resource: &Resource,
    case: &str,
) {
    if let Some(plan) = integration.package_exposure_plan(package, &[resource]) {
        assert!(
            !plan
                .provided_resource_identities
                .contains(&resource.identity()),
            "[{case}] an unsafe declaration must never cover the real resource it collides with, \
             even though a destructive normalizer would have made them match"
        );
    }
    assert_fallback_survives(integration, resource, case);
}

// ============================================================================
// The questions, each asked of every registered harness.
// ============================================================================

/// The fixture every coverage question is asked against: a package holding
/// conventional skills, one skill outside the conventional surface, and a
/// canonical `mcp.json`.
struct Coverage {
    package: StoredPackage,
    resources: Vec<Resource>,
    expected: BTreeSet<String>,
}

fn coverage_fixture(label: &str, skills: &[&str], with_outsider: bool) -> Coverage {
    let (_, package) = build_package(
        label,
        "flow",
        &[("mcp.json", r#"{"mcpServers":{"mcp-a":{"command":"a"}}}"#)],
    );
    let mut resources = Vec::new();
    let mut expected = BTreeSet::new();
    for name in skills {
        let resource = skill_resource(&package, "skills", name);
        expected.insert(resource.identity());
        resources.push(resource);
    }
    if with_outsider {
        resources.push(skill_resource_outside_conventions(&package, "outsider"));
    }
    let mcp = mcp_resource(&package, "mcp-a", r#"{"command":"a"}"#);
    expected.insert(mcp.identity());
    resources.push(mcp);
    Coverage {
        package,
        resources,
        expected,
    }
}

/// A package with an explicit envelope carries no package-level delivery for
/// a harness that has none; asserting that is what keeps "not applicable"
/// from meaning "unasked".
fn assert_no_package_delivery(subject: &Subject, coverage: &Coverage) {
    let refs: Vec<&Resource> = coverage.resources.iter().collect();
    assert!(
        subject
            .integration
            .package_exposure_plan(&coverage.package, &refs)
            .is_none(),
        "[{}] a harness with no package-level envelope must produce no package plan at all",
        subject.id
    );
    for resource in &refs {
        assert_fallback_survives(subject.integration.as_ref(), resource, &subject.id);
    }
}

#[test]
fn every_harness_covers_exactly_what_the_package_declares() {
    for subject in subjects("coverage-full") {
        mark_setup(&subject.home, subject.integration.as_ref());
        let coverage =
            coverage_fixture(&format!("coverage-full-{}", subject.id), &["commit"], false);
        if subject.envelope().get().is_none() {
            assert_no_package_delivery(&subject, &coverage);
            continue;
        }
        let refs: Vec<&Resource> = coverage.resources.iter().collect();
        assert_exact_package_coverage(
            subject.integration.as_ref(),
            &coverage.package,
            &refs,
            &coverage.expected,
            &format!("{}/full", subject.id),
        );
    }
}

#[test]
fn every_harness_covers_only_the_conventional_surface_it_discovered() {
    // Two conventional skills and one discovered outside the conventional
    // surface: the "subset" and "extra discovered" cases fold together, both
    // proven by the same outsider resource.
    for subject in subjects("coverage-subset") {
        mark_setup(&subject.home, subject.integration.as_ref());
        let coverage = coverage_fixture(
            &format!("coverage-subset-{}", subject.id),
            &["commit", "deploy"],
            true,
        );
        if subject.envelope().get().is_none() {
            assert_no_package_delivery(&subject, &coverage);
            continue;
        }
        let refs: Vec<&Resource> = coverage.resources.iter().collect();
        assert_exact_package_coverage(
            subject.integration.as_ref(),
            &coverage.package,
            &refs,
            &coverage.expected,
            &format!("{}/subset", subject.id),
        );
    }
}

#[test]
fn no_harness_turns_a_malformed_envelope_into_coverage() {
    for subject in subjects("malformed") {
        mark_setup(&subject.home, subject.integration.as_ref());
        let Some(envelope) = subject.envelope().get() else {
            continue;
        };
        let (_, package) = build_package(
            &format!("malformed-{}", subject.id),
            "flow",
            &[(envelope.manifest, envelope.malformed)],
        );
        let skill = skill_resource(&package, "skills", "commit");
        assert_a_malformed_envelope_never_becomes_coverage(
            subject.integration.as_ref(),
            &package,
            &skill,
            &format!("{}/malformed", subject.id),
        );
    }
}

#[test]
fn no_harness_lets_an_escaping_declaration_cover_a_resource() {
    for subject in subjects("escaping") {
        mark_setup(&subject.home, subject.integration.as_ref());
        let Some(envelope) = subject.envelope().get() else {
            continue;
        };
        let Some(escaping) = envelope.escaping.get() else {
            continue;
        };
        let (_, package) = build_package(
            &format!("escaping-{}", subject.id),
            "flow",
            &[(envelope.manifest, escaping)],
        );
        let skill = skill_resource(&package, "skills", "commit");
        assert_an_unsafe_declaration_never_covers_the_colliding_resource(
            subject.integration.as_ref(),
            &package,
            &skill,
            &format!("{}/escaping", subject.id),
        );
    }
}

#[test]
fn no_harness_lets_an_absolute_declaration_cover_a_colliding_resource() {
    // The regression this guards: a per-entry normalizer stripped a leading
    // `/` before testing whether a declaration was absolute, so
    // `/skills/commit` silently became the relative `skills/commit` and was
    // accepted. Both vendors that declare paths now share one predicate
    // (`crate::shared::path::normalize_declared_relative_path`); this proves
    // the invariant the fix restores, not either vendor's syntax.
    for subject in subjects("absolute") {
        mark_setup(&subject.home, subject.integration.as_ref());
        let Some(envelope) = subject.envelope().get() else {
            continue;
        };
        let Some(absolute) = envelope.absolute.get() else {
            continue;
        };
        let (_, package) = build_package(
            &format!("absolute-{}", subject.id),
            "flow",
            &[(envelope.manifest, absolute)],
        );
        let skill = skill_resource(&package, "skills", "commit");
        assert_an_unsafe_declaration_never_covers_the_colliding_resource(
            subject.integration.as_ref(),
            &package,
            &skill,
            &format!("{}/absolute", subject.id),
        );
    }
}

#[test]
fn no_harness_lets_a_whitespace_padded_absolute_declaration_cover_a_colliding_resource() {
    // A normalizer that trims before deciding whether a path is absolute
    // reopens the hole the case above closes.
    for subject in subjects("absolute-padded") {
        mark_setup(&subject.home, subject.integration.as_ref());
        let Some(envelope) = subject.envelope().get() else {
            continue;
        };
        let Some(padded) = envelope.absolute_padded.get() else {
            continue;
        };
        let (_, package) = build_package(
            &format!("absolute-padded-{}", subject.id),
            "flow",
            &[(envelope.manifest, padded)],
        );
        let skill = skill_resource(&package, "skills", "commit");
        assert_an_unsafe_declaration_never_covers_the_colliding_resource(
            subject.integration.as_ref(),
            &package,
            &skill,
            &format!("{}/absolute-padded", subject.id),
        );
    }
}

#[test]
fn no_harness_double_counts_a_resource_declared_more_than_once() {
    for subject in subjects("duplicate") {
        mark_setup(&subject.home, subject.integration.as_ref());
        let Some(envelope) = subject.envelope().get() else {
            continue;
        };
        let Some(duplicate) = envelope.duplicate.get() else {
            continue;
        };
        let (_, package) = build_package(
            &format!("duplicate-{}", subject.id),
            "flow",
            &[(envelope.manifest, duplicate)],
        );
        let skill = skill_resource(&package, "skills", "commit");
        let plan = subject
            .integration
            .package_exposure_plan(&package, &[&skill])
            .unwrap_or_else(|| panic!("[{}] explicit envelope present", subject.id));
        assert_eq!(
            plan.provided_resource_identities,
            BTreeSet::from([skill.identity()]),
            "[{}] a repeated declaration must still resolve to exactly one covered identity",
            subject.id
        );
    }
}

#[test]
fn computing_a_package_exposure_plan_is_a_read_on_every_harness() {
    // Planning writes nothing: not into the Store's package tree, and not
    // into the harness's own world — which is stronger than "the generated
    // directory was not created" (the vendor-private assertion this
    // replaces), and holds for a harness that generates nothing at all.
    // Bytes and symlink targets, not merely the set of paths.
    let coverage = coverage_fixture("plan-is-a-read", &["commit"], false);
    let refs: Vec<&Resource> = coverage.resources.iter().collect();
    let store_before = tree(&coverage.package.root);
    for subject in subjects("plan-is-a-read") {
        mark_setup(&subject.home, subject.integration.as_ref());
        let world_before = tree(&subject.root);
        let _ = subject
            .integration
            .package_exposure_plan(&coverage.package, &refs);
        assert_eq!(
            store_before,
            tree(&coverage.package.root),
            "[{}] planning must never mutate the Store's own package tree",
            subject.id
        );
        assert_eq!(
            world_before,
            tree(&subject.root),
            "[{}] planning must write nothing into the harness's world either",
            subject.id
        );
    }
}

#[test]
fn no_harness_tops_up_an_explicit_envelopes_own_declaration_with_generation() {
    // An explicit envelope that declares LESS than generation would: presence
    // of the envelope must keep coverage exactly at what it declares, proving
    // generation is never consulted once an envelope exists.
    for subject in subjects("precedence") {
        mark_setup(&subject.home, subject.integration.as_ref());
        let Some(envelope) = subject.envelope().get() else {
            continue;
        };
        let mut files = vec![(
            "mcp.json",
            r#"{"mcpServers":{"mcp-a":{"command":"a"},"mcp-b":{"command":"b"}}}"#,
        )];
        files.extend_from_slice(envelope.partial.files);
        let (_, package) = build_package(&format!("precedence-{}", subject.id), "flow", &files);

        let skill = skill_resource(&package, "skills", "commit");
        let mcp_a = mcp_resource(&package, "mcp-a", r#"{"command":"a"}"#);
        let mcp_b = mcp_resource(&package, "mcp-b", r#"{"command":"b"}"#);
        let named = [("skill", &skill), ("mcp-a", &mcp_a), ("mcp-b", &mcp_b)];
        let expected: BTreeSet<String> = envelope
            .partial
            .declares
            .iter()
            .map(|name| {
                named
                    .iter()
                    .find(|(fixture, _)| fixture == name)
                    .unwrap_or_else(|| panic!("{name:?} is not a fixture resource"))
                    .1
                    .identity()
            })
            .collect();

        let refs: Vec<&Resource> = named.iter().map(|(_, resource)| *resource).collect();
        assert_exact_package_coverage(
            subject.integration.as_ref(),
            &package,
            &refs,
            &expected,
            &format!("{}/precedence", subject.id),
        );
    }
}

#[test]
fn every_exemption_from_this_suite_states_its_reason() {
    // The point of a declared exemption: it is readable, and it is a claim
    // someone can be wrong about. An empty reason is an omission wearing a
    // declaration's clothes.
    for subject in subjects("exemptions") {
        let mut declared = Vec::new();
        if let Some(reason) = subject.envelope().reason() {
            declared.push(("package envelope", reason));
        } else {
            let envelope = subject.envelope().get().expect("checked");
            for (question, support) in [
                ("escaping declaration", envelope.escaping.reason()),
                ("absolute declaration", envelope.absolute.reason()),
                (
                    "padded absolute declaration",
                    envelope.absolute_padded.reason(),
                ),
                ("duplicate declaration", envelope.duplicate.reason()),
            ] {
                if let Some(reason) = support {
                    declared.push((question, reason));
                }
            }
        }
        for (question, reason) in declared {
            assert!(
                reason.len() > 20,
                "[{}] the exemption from {question} must say why, not merely that it exists: {reason:?}",
                subject.id
            );
        }
    }
}

/// Fixture roots are removed when a `Subject` drops; the package roots the
/// coverage fixtures build are scratch directories of their own.
#[test]
fn every_registered_harness_is_a_conformance_subject() {
    let found: Vec<String> = subjects("registry").iter().map(|s| s.id.clone()).collect();
    assert!(
        found.len() >= 4,
        "the registry produced {found:?}; a harness that cannot be constructed in isolation \
         cannot be conformance-tested either"
    );
    let mut sorted = found.clone();
    sorted.sort();
    sorted.dedup();
    assert_eq!(
        sorted.len(),
        found.len(),
        "duplicate harness ids: {found:?}"
    );
    let _ = fs::metadata(".");
}
