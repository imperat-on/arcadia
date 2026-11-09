use super::*;
use serde_json::json;

fn manifest(appid: &str, name: &str, buildid: &str, flags: u64) -> AcfManifest {
    AcfManifest {
        appid: appid.to_string(),
        name: name.to_string(),
        installdir: format!("Dir {appid}"),
        buildid: buildid.to_string(),
        state_flags: flags,
        size_on_disk: 100,
        last_updated: 50,
        last_played: 25,
        arcadia_download: false,
    }
}

#[test]
fn state_flags_map_to_the_documented_statuses() {
    assert_eq!(state_flags_status(0), UpdateStatus::Unknown);
    assert_eq!(
        state_flags_status(STATE_UNINSTALLED),
        UpdateStatus::NotInstalled
    );
    assert_eq!(
        state_flags_status(STATE_UPDATE_REQUIRED),
        UpdateStatus::UpdateAvailable
    );
    assert_eq!(
        state_flags_status(STATE_FULLY_INSTALLED),
        UpdateStatus::UpToDate
    );
    assert_eq!(
        state_flags_status(STATE_UPDATE_QUEUED),
        UpdateStatus::UpdateAvailable
    );
    assert_eq!(
        state_flags_status(STATE_UPDATE_OPTIONAL),
        UpdateStatus::UpdateAvailable
    );
    assert_eq!(
        state_flags_status(STATE_FILES_MISSING),
        UpdateStatus::FilesMissing
    );
    assert_eq!(
        state_flags_status(STATE_FILES_CORRUPT),
        UpdateStatus::FilesCorrupt
    );
    assert_eq!(
        state_flags_status(STATE_UPDATE_RUNNING),
        UpdateStatus::Downloading
    );
    assert_eq!(
        state_flags_status(STATE_UPDATE_STARTED),
        UpdateStatus::Downloading
    );
    assert_eq!(
        state_flags_status(STATE_DOWNLOADING),
        UpdateStatus::Downloading
    );
    assert_eq!(
        state_flags_status(STATE_VALIDATING),
        UpdateStatus::Verifying
    );
    assert_eq!(
        state_flags_status(STATE_UNINSTALLING),
        UpdateStatus::Uninstalling
    );
    assert_eq!(
        state_flags_status(STATE_SHARED_ONLY),
        UpdateStatus::NotInstalled
    );
    assert_eq!(
        state_flags_status(STATE_FULLY_INSTALLED | STATE_DOWNLOADING),
        UpdateStatus::Downloading
    );
    assert_eq!(
        state_flags_status(STATE_FULLY_INSTALLED | STATE_UPDATE_PAUSED),
        UpdateStatus::UpdateAvailable
    );
    assert!(UpdateStatus::UpdateAvailable.needs_update());
    assert!(UpdateStatus::FilesMissing.needs_update());
    assert!(!UpdateStatus::Downloading.needs_update());
    assert!(!UpdateStatus::UpToDate.needs_update());
}

#[test]
fn buildids_compare_numerically_then_textually() {
    assert!(buildid_is_newer("123", "124"));
    assert!(!buildid_is_newer("124", "123"));
    assert!(!buildid_is_newer("124", "124"));
    assert!(buildid_is_newer("", "1"));
    assert!(!buildid_is_newer("1", ""));
    assert!(buildid_is_newer("abc", "abd"));
    assert!(!buildid_is_newer("abc", "abc"));
    assert_eq!(
        buildid_of(&manifest("730", "CS2", "42", STATE_FULLY_INSTALLED)),
        "42"
    );
}

#[test]
fn update_requires_a_newer_buildid_and_handles_missing_latest() {
    let app = manifest("730", "Counter-Strike 2", "100", STATE_FULLY_INSTALLED);
    let newer = build_update(&app, Some(&LatestBuild::new("101", "public")));
    assert_eq!(newer.status, UpdateStatus::UpdateAvailable);
    assert!(newer.needs_update);
    assert_eq!(newer.latest_buildid.as_deref(), Some("101"));
    assert_eq!(newer.title, "Counter-Strike 2");
    assert_eq!(newer.installdir, "Dir 730");
    assert_eq!(newer.size_on_disk, 100);

    let current = build_update(&app, Some(&LatestBuild::new("100", "public")));
    assert_eq!(current.status, UpdateStatus::UpToDate);
    assert!(!current.needs_update);

    let unknown = build_update(&app, None);
    assert_eq!(unknown.status, UpdateStatus::Unknown);
    assert!(!unknown.needs_update);
    assert_eq!(unknown.latest_buildid, None);

    let no_installed_buildid = build_update(
        &manifest("730", "X", "", STATE_FULLY_INSTALLED),
        Some(&LatestBuild::new("1", "public")),
    );
    assert_eq!(no_installed_buildid.status, UpdateStatus::Unknown);
}

#[test]
fn update_flags_win_over_an_equal_buildid() {
    let app = manifest(
        "730",
        "CS2",
        "100",
        STATE_FULLY_INSTALLED | STATE_UPDATE_REQUIRED,
    );
    let update = build_update(&app, Some(&LatestBuild::new("100", "public")));
    assert_eq!(update.status, UpdateStatus::UpdateAvailable);
    assert!(update.needs_update);

    let downloading = build_update(
        &manifest(
            "730",
            "CS2",
            "100",
            STATE_FULLY_INSTALLED | STATE_DOWNLOADING,
        ),
        Some(&LatestBuild::new("101", "public")),
    );
    assert_eq!(downloading.status, UpdateStatus::Downloading);
    assert!(!downloading.needs_update);
}

#[test]
fn report_counts_and_filters_not_installed_by_default() {
    let apps = vec![
        manifest("1", "Up to date", "10", STATE_FULLY_INSTALLED),
        manifest("2", "Update", "10", STATE_FULLY_INSTALLED),
        manifest("3", "Missing", "0", STATE_UNINSTALLED),
        manifest("4", "Required", "10", STATE_UPDATE_REQUIRED),
        manifest(
            "5",
            "Downloading",
            "10",
            STATE_FULLY_INSTALLED | STATE_DOWNLOADING,
        ),
    ];
    let mut latest = BTreeMap::new();
    latest.insert("1".to_string(), LatestBuild::new("10", "public"));
    latest.insert("2".to_string(), LatestBuild::new("11", "public"));
    latest.insert("4".to_string(), LatestBuild::new("10", "public"));
    latest.insert("5".to_string(), LatestBuild::new("11", "public"));

    let report = update_report(&apps, &latest, &CheckUpdatesOptions::default());
    assert!(report.ok);
    assert_eq!(report.total, 4);
    assert_eq!(report.pendentes, 2);
    assert_eq!(report.atualizados, 1);
    assert_eq!(
        report
            .updates
            .iter()
            .map(|update| update.appid.as_str())
            .collect::<Vec<_>>(),
        vec!["1", "2", "4", "5"]
    );

    let report = update_report(
        &apps,
        &latest,
        &CheckUpdatesOptions {
            branch: "public".to_string(),
            include_not_installed: true,
        },
    );
    assert_eq!(report.total, 5);
    assert!(report
        .updates
        .iter()
        .any(|update| update.status == UpdateStatus::NotInstalled));

    assert_eq!(
        check_updates(&apps, &latest, &CheckUpdatesOptions::default()),
        update_report(&apps, &latest, &CheckUpdatesOptions::default())
    );
}

#[test]
fn report_serialization_omits_latest_when_unknown_and_has_failure_envelope() {
    let report = update_report(
        &[manifest("730", "X", "1", STATE_FULLY_INSTALLED)],
        &BTreeMap::new(),
        &CheckUpdatesOptions::default(),
    );
    let value = serde_json::to_value(&report).unwrap();
    assert_eq!(value["ok"], true);
    assert_eq!(value["total"], 1);
    assert_eq!(value["pendentes"], 0);
    assert_eq!(value["atualizados"], 0);
    assert_eq!(value["updates"][0]["status"], "unknown");
    assert!(value["updates"][0].get("latest_buildid").is_none());

    let failure = UpdateReport::failure("catálogo fora");
    let value = serde_json::to_value(&failure).unwrap();
    assert_eq!(value["ok"], false);
    assert_eq!(value["error"], "catálogo fora");
}

#[test]
fn latest_build_from_app_info_uses_the_requested_branch() {
    let info = crate::depot::parse_app_info(
        &json!({"data": {"730": {
            "common": {"name": "CS2"},
            "depots": {"branches": {
                "public": {"buildid": "100"},
                "beta": {"buildid": "200"}
            }}
        }}}),
        "730",
    )
    .unwrap();
    assert_eq!(
        latest_from_app_info(&info, "public"),
        Some(LatestBuild::new("100", "public"))
    );
    assert_eq!(
        latest_from_app_info(&info, "beta"),
        Some(LatestBuild::new("200", "beta"))
    );
    assert_eq!(latest_from_app_info(&info, "missing"), None);
}
