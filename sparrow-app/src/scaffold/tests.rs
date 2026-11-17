use super::{engine::*, feature};
use std::{fs, path::Path};

const PUBSPEC: &str = "# Team conventions\nname: example_app\nenvironment:\n  sdk: '>=3.10.0 <4.0.0'\ndependencies: # Keep this comment\n  flutter:\n    sdk: flutter\n";
fn project() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("pubspec.yaml"), PUBSPEC).unwrap();
    dir
}
fn ctx(root: &Path) -> Context {
    context(root, root).unwrap()
}
fn custom() -> Pack {
    serde_yaml::from_str("schema: 1\nid: team.model\nversion: 1.0.0\ntitle: Model\nfiles:\n  - path: lib/{{snake}}.dart\n    content: 'class {{name}} {}'\ndependencies:\n  - package: json_annotation\n    version: ^4.9.0\n").unwrap()
}
#[test]
fn feature_preview_apply_idempotency_and_undo_are_one_change() {
    let dir = project();
    let context = ctx(dir.path());
    let plan = plan_pack(&context, &feature::pack(), "Orders").unwrap();
    assert_eq!(plan.changes.len(), 8);
    assert!(
        !dir.path().join("lib").exists(),
        "preview must be read-only"
    );
    assert!(plan.diff(0).contains("+class OrdersItem"));
    let applied = plan.apply().unwrap();
    for change in &plan.changes {
        assert_eq!(
            fs::read_to_string(&change.path).unwrap(),
            change.after.as_ref().unwrap().as_str()
        );
    }
    assert!(
        plan_pack(&ctx(dir.path()), &feature::pack(), "Orders")
            .unwrap()
            .changes
            .is_empty()
    );
    applied.undo().unwrap();
    assert_eq!(
        fs::read_to_string(dir.path().join("pubspec.yaml")).unwrap(),
        PUBSPEC
    );
    assert!(!dir.path().join("lib").exists());
    assert!(!dir.path().join("test").exists());
}
#[test]
fn conflicts_abort_before_any_write_and_preserve_user_code() {
    let dir = project();
    let context = ctx(dir.path());
    let plan = plan_pack(&context, &feature::pack(), "Orders").unwrap();
    let conflict = &plan.changes[2].path;
    fs::create_dir_all(conflict.parent().unwrap()).unwrap();
    fs::write(conflict, "user code").unwrap();
    assert!(plan.apply().unwrap_err().contains("Changed since preview"));
    assert!(!plan.changes[0].path.exists());
    assert_eq!(fs::read_to_string(conflict).unwrap(), "user code");
    assert!(
        plan_pack(&context, &feature::pack(), "Orders")
            .unwrap_err()
            .contains("Already exists")
    );
}
#[test]
fn undo_refuses_to_remove_edited_generated_files() {
    let dir = project();
    let plan = plan_pack(&ctx(dir.path()), &feature::pack(), "Orders").unwrap();
    let applied = plan.apply().unwrap();
    fs::write(&plan.changes[2].path, "user additions").unwrap();
    assert!(applied.undo().is_err());
    assert!(plan.changes[0].path.exists());
    assert_eq!(
        fs::read_to_string(&plan.changes[2].path).unwrap(),
        "user additions"
    );
    assert!(
        fs::read_to_string(dir.path().join("pubspec.yaml"))
            .unwrap()
            .contains("flutter_test")
    );
}
#[test]
fn project_settings_are_snapshot_inputs_even_when_not_edited() {
    let dir = project();
    let pack = Pack {
        dependencies: vec![],
        ..custom()
    };
    let plan = plan_pack(&ctx(dir.path()), &pack, "Orders").unwrap();
    fs::write(
        dir.path().join("pubspec.yaml"),
        PUBSPEC.replace("example_app", "renamed_app"),
    )
    .unwrap();
    assert!(plan.apply().unwrap_err().contains("settings changed"));
    assert!(!dir.path().join("lib").exists());
}
#[test]
fn nearest_package_and_explicit_layout_override_win() {
    let dir = project();
    let child = dir.path().join("packages/child");
    fs::create_dir_all(child.join("lib/presentation")).unwrap();
    fs::write(
        child.join("pubspec.yaml"),
        PUBSPEC.replace("example_app", "child"),
    )
    .unwrap();
    let child_ctx = context(dir.path(), &child.join("lib/presentation")).unwrap();
    assert_eq!(child_ctx.package, "child");
    assert_eq!(child_ctx.layout, "layer-first");
    let plan = plan_pack(&child_ctx, &feature::pack(), "Orders").unwrap();
    assert!(
        plan.changes
            .iter()
            .any(|c| c.path == child.join("lib/data/orders/orders_repository.dart"))
    );
    assert!(plan.changes.iter().all(|c| c.path.starts_with(&child)));
    fs::create_dir_all(child.join(".sparrow")).unwrap();
    fs::write(
        child.join(".sparrow/scaffold.yaml"),
        "layout: feature-first\n",
    )
    .unwrap();
    assert_eq!(context(dir.path(), &child).unwrap().layout, "feature-first");
}
#[test]
fn singular_feature_convention_keeps_correct_package_imports() {
    let dir = project();
    fs::create_dir_all(dir.path().join("lib/feature")).unwrap();
    let plan = plan_pack(&ctx(dir.path()), &feature::pack(), "Orders").unwrap();
    let screen = plan
        .changes
        .iter()
        .find(|c| c.path.ends_with("orders_screen.dart"))
        .unwrap();
    assert!(
        screen
            .path
            .ends_with("lib/feature/orders/presentation/orders_screen.dart")
    );
    assert!(
        screen.after.as_ref().unwrap().contains(
            "package:example_app/feature/orders/data/orders_repository.dart"
        )
    );
}
#[test]
fn dependencies_preserve_comments_constraints_and_report_version_drift() {
    let dir = project();
    let before = format!("{PUBSPEC}  json_annotation: ^3.0.0\n");
    fs::write(dir.path().join("pubspec.yaml"), &before).unwrap();
    fs::write(
        dir.path().join("pubspec.lock"),
        "packages:\n  json_annotation:\n    version: '3.1.0'\n",
    )
    .unwrap();
    let plan = plan_pack(&ctx(dir.path()), &custom(), "Orders").unwrap();
    assert_eq!(plan.changes.len(), 1);
    assert!(
        plan.notes
            .iter()
            .any(|n| n.contains("Compatibility warning"))
    );
    plan.apply().unwrap();
    assert_eq!(
        fs::read_to_string(dir.path().join("pubspec.yaml")).unwrap(),
        before
    );
}
#[test]
fn manifest_dependency_additions_are_previewed_and_restored() {
    let dir = project();
    let mut pack = custom();
    pack.dependencies.push(Dependency {
        package: "mocktail".into(),
        version: Some("^1.0.0".into()),
        sdk: None,
        dev: true,
    });
    pack.dependencies.push(Dependency {
        package: "collection".into(),
        version: Some(">=1.18.0 <2.0.0".into()),
        sdk: None,
        dev: false,
    });
    let plan = plan_pack(&ctx(dir.path()), &pack, "Orders").unwrap();
    let diff = plan.diff(1);
    assert!(diff.contains("+  json_annotation: '^4.9.0'"));
    assert!(diff.contains("dependencies: # Keep this comment"));
    let applied = plan.apply().unwrap();
    let text = fs::read_to_string(dir.path().join("pubspec.yaml")).unwrap();
    assert!(text.contains("dependencies: # Keep this comment"));
    let yaml: serde_yaml::Value = serde_yaml::from_str(&text).unwrap();
    assert_eq!(
        yaml["dependencies"]["collection"].as_str(),
        Some(">=1.18.0 <2.0.0")
    );
    assert_eq!(
        yaml["dev_dependencies"]["mocktail"].as_str(),
        Some("^1.0.0")
    );
    applied.undo().unwrap();
    assert_eq!(
        fs::read_to_string(dir.path().join("pubspec.yaml")).unwrap(),
        PUBSPEC
    );
}
#[test]
fn invalid_packs_and_unsafe_destinations_are_rejected() {
    let dir = project();
    let context = ctx(dir.path());
    for path in [
        "../outside.dart",
        "/tmp/outside.dart",
        ".git/refs/new",
        "pubspec.yaml",
        ".sparrow/settings.toml",
    ] {
        let mut pack = custom();
        pack.files[0].path = path.into();
        assert!(plan_pack(&context, &pack, "Orders").is_err(), "{path}");
    }
    let mut pack = custom();
    pack.files.push(pack.files[0].clone());
    assert!(plan_pack(&context, &pack, "Orders").is_err());
    pack = custom();
    pack.files[0].content = "{{unknown}}".into();
    assert!(plan_pack(&context, &pack, "Orders").is_err());
    pack = custom();
    pack.schema = 2;
    assert!(plan_pack(&context, &pack, "Orders").is_err());
    assert!(plan_pack(&context, &custom(), "../../escape").is_err());
    let manifest = dir.path().join("pack.yaml");
    fs::write(
        &manifest,
        "schema: 1\nid: bad\nversion: 1.0.0\ntitle: Bad\nfiles: []\nhooks: [run]\n",
    )
    .unwrap();
    assert!(load_pack(&manifest).unwrap_err().contains("unknown field"));
}
#[cfg(unix)]
#[test]
fn symlinks_introduced_after_preview_and_dangling_files_are_rejected() {
    use std::os::unix::fs::symlink;
    let dir = project();
    let outside = tempfile::tempdir().unwrap();
    let plan = plan_pack(&ctx(dir.path()), &custom(), "Orders").unwrap();
    symlink(outside.path(), dir.path().join("lib")).unwrap();
    assert!(plan.apply().is_err());
    assert!(!outside.path().join("orders.dart").exists());
    fs::remove_file(dir.path().join("lib")).unwrap();
    fs::create_dir(dir.path().join("lib")).unwrap();
    symlink(
        outside.path().join("missing"),
        dir.path().join("lib/orders.dart"),
    )
    .unwrap();
    assert!(plan_pack(&ctx(dir.path()), &custom(), "Orders").is_err());
}
#[test]
fn preparation_failure_leaves_all_files_and_directories_unchanged() {
    let dir = project();
    let mut pack = custom();
    pack.dependencies.clear();
    pack.files.push(PackFile {
        path: "blocked/second.dart".into(),
        content: "class Second {}".into(),
    });
    let plan = plan_pack(&ctx(dir.path()), &pack, "Orders").unwrap();
    fs::write(dir.path().join("blocked"), "not a folder").unwrap();
    assert!(plan.apply().is_err());
    assert!(!dir.path().join("lib").exists());
    assert_eq!(
        fs::read_to_string(dir.path().join("blocked")).unwrap(),
        "not a folder"
    );
}
#[test]
fn empty_dependency_sections_are_supported_without_rewriting_yaml() {
    let dir = project();
    let before = format!("{PUBSPEC}dev_dependencies:");
    fs::write(dir.path().join("pubspec.yaml"), &before).unwrap();
    let plan = plan_pack(&ctx(dir.path()), &feature::pack(), "Orders").unwrap();
    plan.apply().unwrap();
    assert!(
        fs::read_to_string(dir.path().join("pubspec.yaml"))
            .unwrap()
            .contains("dev_dependencies:\n  flutter_test:\n    sdk: flutter\n")
    );
}
#[test]
#[ignore = "exports complete feature fixtures for real Flutter SDK analysis and tests"]
fn export_features_for_sdk_validation() {
    let root = std::path::PathBuf::from(
        std::env::var_os("SPARROW_FEATURE_EXPORT_ROOT")
            .expect("isolated fixture root"),
    );
    for layout in ["feature-first", "layer-first"] {
        let dir = root.join(layout);
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("pubspec.yaml"), PUBSPEC).unwrap();
        if layout == "layer-first" {
            fs::create_dir_all(dir.join("lib/presentation")).unwrap();
        }
        plan_pack(&ctx(&dir), &feature::pack(), "Orders")
            .unwrap()
            .apply()
            .unwrap();
    }
}
