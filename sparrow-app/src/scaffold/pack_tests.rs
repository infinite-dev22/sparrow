use super::{catalog, engine::*};
use std::{fs, path::Path};
fn project() -> tempfile::TempDir {
    let d = tempfile::tempdir().unwrap();
    fs::write(d.path().join("pubspec.yaml"), "name: example_app\n").unwrap();
    d
}
fn context(root: &Path) -> Context {
    super::engine::context(root, root).unwrap()
}
fn write_pack(root: &Path, folder: &str, title: &str) -> std::path::PathBuf {
    let dir = root.join(folder);
    fs::create_dir_all(&dir).unwrap();
    let file = dir.join("pack.yaml");
    fs::write(&file,format!("schema: 2\nid: sparrow.state.cubit\nversion: 1.0.0\ntitle: {title}\ntarget: sibling\nsuffix: _cubit\nfiles:\n  - path: '{{{{suffixed_snake}}}}.dart'\n    content: 'class {{{{suffixed_name}}}} {{}}'\n")).unwrap();
    file
}
#[test]
fn bundled_single_file_output_matches_pre_migration_bytes() {
    use sha2::{Digest, Sha256};
    let golden: serde_json::Value =
        serde_json::from_str(include_str!("bundled-golden.json")).unwrap();
    let root = project();
    let ctx = context(root.path());
    let entries = catalog::bundled_entries();
    assert_eq!(entries.len(), 22);
    for entry in entries {
        let pack = entry.load().unwrap();
        if pack.target == Target::Sibling {
            let plan = plan_sibling(&ctx, root.path(), &pack, "Counter").unwrap();
            let file = &plan.changes[0];
            assert_eq!(
                file.path.file_name().unwrap().to_str().unwrap(),
                golden[&pack.id]["path"].as_str().unwrap()
            );
            assert_eq!(
                format!(
                    "{:x}",
                    Sha256::digest(file.after.as_ref().unwrap().as_bytes())
                ),
                golden[&pack.id]["sha256"].as_str().unwrap(),
                "{}",
                pack.id
            );
        }
    }
}
#[test]
fn feature_pack_output_matches_every_pre_migration_file() {
    use sha2::{Digest, Sha256};
    let golden: serde_json::Value =
        serde_json::from_str(include_str!("bundled-golden.json")).unwrap();
    let root = project();
    let plan = plan_pack(
        &context(root.path()),
        &catalog::bundled("sparrow.feature.mvvm").unwrap(),
        "Orders",
    )
    .unwrap();
    let generated = plan
        .changes
        .iter()
        .filter(|c| c.path.extension().is_some_and(|s| s == "dart"))
        .collect::<Vec<_>>();
    assert_eq!(generated.len(), 7);
    for file in generated {
        let path = file
            .path
            .strip_prefix(root.path())
            .unwrap()
            .to_str()
            .unwrap();
        assert_eq!(
            format!(
                "{:x}",
                Sha256::digest(file.after.as_ref().unwrap().as_bytes())
            ),
            golden["sparrow.feature.mvvm"]["files"][path]
                .as_str()
                .unwrap(),
            "{path}"
        );
    }
}
#[test]
fn global_source_snapshots_are_validated_outside_the_output_package() {
    let root = project();
    let global = tempfile::tempdir().unwrap();
    let path = write_pack(global.path(), "global", "User");
    let pack = load_pack(&path).unwrap();
    let plan =
        plan_sibling(&context(root.path()), root.path(), &pack, "Counter").unwrap();
    assert!(plan.validate().is_ok());
    fs::write(
        &path,
        fs::read_to_string(&path)
            .unwrap()
            .replace("title: User", "title: Updated"),
    )
    .unwrap();
    assert!(plan.apply().is_err());
    assert!(!root.path().join("counter_cubit.dart").exists());
}
#[test]
fn project_overrides_global_overrides_bundled_and_removal_restores_lower_layer() {
    let root = project();
    let global = tempfile::tempdir().unwrap();
    let g = write_pack(global.path(), "user-cubit", "User");
    let local = root.path().join(".sparrow/scaffolds");
    let p = write_pack(&local, "team-cubit", "Project");
    let get = || {
        catalog::discover_at(Some(global.path()), root.path())
            .into_iter()
            .find(|e| e.id == "sparrow.state.cubit")
            .unwrap()
    };
    assert_eq!(get().title, "Project");
    assert!(get().provenance.starts_with("Project:"));
    fs::remove_file(p).unwrap();
    assert_eq!(get().title, "User");
    fs::remove_file(g).unwrap();
    assert_eq!(get().title, "Cubit");
}
#[test]
fn broken_project_override_is_visible_and_never_falls_back() {
    let root = project();
    let path = write_pack(
        &root.path().join(".sparrow/scaffolds"),
        "sparrow.state.cubit",
        "Project",
    );
    fs::write(&path, "schema: 2\nid: sparrow.state.cubit\ntitle: Broken\n").unwrap();
    let e = catalog::discover_at(None, root.path())
        .into_iter()
        .find(|e| e.id == "sparrow.state.cubit")
        .unwrap();
    assert!(e.load().is_err());
    assert!(e.title.starts_with("Invalid"));
}
#[test]
fn duplicate_local_ids_and_disabled_overrides_are_not_silent() {
    let root = project();
    let local = root.path().join(".sparrow/scaffolds");
    write_pack(&local, "a", "First");
    let second = write_pack(&local, "b", "Second");
    let e = catalog::discover_at(None, root.path())
        .into_iter()
        .find(|e| e.id == "sparrow.state.cubit")
        .unwrap();
    assert!(e.load().unwrap_err().contains("Duplicate"));
    fs::remove_file(second).unwrap();
    let first = local.join("a/pack.yaml");
    let text = fs::read_to_string(&first).unwrap();
    fs::write(first, format!("{text}disabled: true\n")).unwrap();
    assert!(
        !catalog::discover_at(None, root.path())
            .iter()
            .any(|e| e.id == "sparrow.state.cubit")
    );
}
#[test]
fn multi_file_sibling_pack_uses_selected_folder_and_undo() {
    let root = project();
    let base = root.path().join("lib/counters");
    fs::create_dir_all(&base).unwrap();
    let manifest = write_pack(root.path(), "pack", "Counter");
    let text = fs::read_to_string(&manifest).unwrap();
    fs::write(&manifest,format!("{text}  - path: '{{{{suffixed_snake}}}}_test.dart'\n    content: 'void main() {{}}'\n")).unwrap();
    let pack = load_pack(&manifest).unwrap();
    let ctx = context(root.path());
    let plan = plan_sibling(&ctx, &base, &pack, "CounterCubit").unwrap();
    assert_eq!(plan.changes.len(), 2);
    assert!(
        plan.changes
            .iter()
            .all(|c| c.path.parent() == Some(base.as_path()))
    );
    assert!(
        plan.changes[0]
            .after
            .as_ref()
            .unwrap()
            .contains("CounterCubit {}")
    );
    let applied = plan.apply().unwrap();
    applied.undo().unwrap();
    assert!(!base.join("counter_cubit.dart").exists());
    assert!(plan_pack(&ctx, &pack, "Counter").is_err());
}
#[test]
fn source_files_are_reloaded_and_changes_invalidate_preview() {
    let root = project();
    let dir = root.path().join("pack");
    fs::create_dir(&dir).unwrap();
    let path = dir.join("pack.yaml");
    fs::write(&path,"schema: 2\nid: custom.source\nversion: 1.0.0\ntitle: Source\nfiles:\n  - path: lib/{{snake}}.dart\n    source: body.tmpl\n").unwrap();
    let body = dir.join("body.tmpl");
    fs::write(&body, "class {{name}} {}\n").unwrap();
    let ctx = context(root.path());
    let plan = plan_pack(&ctx, &load_pack(&path).unwrap(), "Counter").unwrap();
    fs::write(&body, "class {{name}} { final int value=1; }\n").unwrap();
    assert!(plan.apply().is_err());
    assert!(!root.path().join("lib").exists());
    assert!(
        plan_pack(&ctx, &load_pack(&path).unwrap(), "Counter")
            .unwrap()
            .changes[0]
            .after
            .as_ref()
            .unwrap()
            .contains("value=1")
    );
    fs::write(
        &path,
        fs::read_to_string(&path)
            .unwrap()
            .replace("body.tmpl", "../outside"),
    )
    .unwrap();
    assert!(load_pack(&path).is_err());
}
#[test]
fn schema_one_packs_keep_working_and_registration_is_explicit() {
    let root = project();
    let path = write_pack(root.path(), "pack", "Custom");
    let text = fs::read_to_string(&path).unwrap();
    fs::write(&path, text.replace("schema: 2", "schema: 1")).unwrap();
    let pack = load_pack(&path).unwrap();
    assert!(pack.registration.is_none());
    fs::write(&path, format!("{text}registration: mvvm_change_notifier\n")).unwrap();
    assert!(load_pack(&path).unwrap_err().contains("package-target"));
}
#[test]
fn suffix_variables_do_not_duplicate_multiword_suffixes() {
    let root = project();
    let path = write_pack(root.path(), "pack", "Custom");
    let text = fs::read_to_string(&path).unwrap();
    fs::write(&path, text.replace("_cubit", "_view_model")).unwrap();
    let pack = load_pack(&path).unwrap();
    let ctx = context(root.path());
    for name in ["Counter", "CounterViewModel"] {
        let p = plan_sibling(&ctx, root.path(), &pack, name).unwrap();
        assert!(p.changes[0].path.ends_with("counter_view_model.dart"));
        assert_eq!(
            p.changes[0].after.as_deref(),
            Some("class CounterViewModel {}")
        );
    }
}
#[test]
fn whitespace_variables_and_adjacent_dart_braces_render_without_escaping() {
    let root = project();
    let path = write_pack(root.path(), "pack", "Custom");
    let text = fs::read_to_string(&path).unwrap().replace(
        "class {{suffixed_name}} {}",
        "void use({{{ name }}? value}) {} ",
    );
    fs::write(&path, text).unwrap();
    let pack = load_pack(&path).unwrap();
    let p =
        plan_sibling(&context(root.path()), root.path(), &pack, "Counter").unwrap();
    assert_eq!(
        p.changes[0].after.as_deref(),
        Some("void use({Counter? value}) {} ")
    );
}
#[test]
fn oversize_manifest_and_external_source_are_rejected_before_rendering() {
    let root = project();
    let path = write_pack(root.path(), "pack", "Custom");
    let original = fs::read_to_string(&path).unwrap();
    fs::write(&path, "x".repeat(2 * 1024 * 1024 + 1)).unwrap();
    assert!(load_pack(&path).unwrap_err().contains("2 MB"));
    fs::write(
        &path,
        original
            .replace("content: 'class {{suffixed_name}} {}'", "source: huge.tmpl"),
    )
    .unwrap();
    fs::write(
        path.parent().unwrap().join("huge.tmpl"),
        "x".repeat(2 * 1024 * 1024 + 1),
    )
    .unwrap();
    assert!(load_pack(&path).unwrap_err().contains("2 MB"));
}
#[test]
fn compatibility_metadata_warns_without_changing_dependencies() {
    let root = project();
    fs::write(
        root.path().join("pubspec.lock"),
        "packages:\n  bloc:\n    version: 8.0.0\n",
    )
    .unwrap();
    let path = write_pack(root.path(), "pack", "Custom");
    let text = fs::read_to_string(&path).unwrap();
    fs::write(&path, format!("{text}compatible_with:\n  bloc: ^9.0.0\n")).unwrap();
    let p = plan_sibling(
        &context(root.path()),
        root.path(),
        &load_pack(&path).unwrap(),
        "Counter",
    )
    .unwrap();
    assert!(
        p.notes
            .iter()
            .any(|s| s.contains("Compatibility warning: bloc"))
    );
    assert_eq!(p.changes.len(), 1);
}
#[test]
fn export_is_previewable_conflict_safe_and_undoable() {
    let root = project();
    let ctx = context(root.path());
    let files = catalog::export_files("sparrow.feature.mvvm").unwrap();
    let plan = plan_export(&ctx, files.clone()).unwrap();
    assert!(!root.path().join(".sparrow").exists());
    let applied = plan.apply().unwrap();
    assert!(
        load_pack(
            &root
                .path()
                .join(".sparrow/scaffolds/sparrow.feature.mvvm/pack.yaml")
        )
        .unwrap()
        .registration
        .is_some()
    );
    assert!(plan_export(&ctx, files.clone()).unwrap().changes.is_empty());
    applied.undo().unwrap();
    assert!(!root.path().join(".sparrow").exists());
    plan.apply().unwrap();
    fs::write(
        root.path()
            .join(".sparrow/scaffolds/sparrow.feature.mvvm/pack.yaml"),
        "user override",
    )
    .unwrap();
    assert!(plan_export(&ctx, files).is_err());
}
#[test]
fn legacy_templates_still_enter_the_same_planner() {
    let root = project();
    fs::create_dir_all(root.path().join(".sparrow/templates")).unwrap();
    fs::write(
        root.path().join(".sparrow/templates/custom.dart.tmpl"),
        "class {{name}} {}\n",
    )
    .unwrap();
    let e = catalog::discover_at(None, root.path())
        .into_iter()
        .find(|e| e.id == "local.template.custom")
        .unwrap();
    let p = plan_sibling(
        &context(root.path()),
        root.path(),
        &e.load().unwrap(),
        "Example",
    )
    .unwrap();
    assert!(p.changes[0].path.ends_with("example.dart"));
}
#[cfg(unix)]
#[test]
fn external_template_symlinks_and_protected_sibling_destinations_are_rejected() {
    use std::os::unix::fs::symlink;
    let root = project();
    let outside = project();
    let path = write_pack(root.path(), "pack", "Custom");
    let text = fs::read_to_string(&path).unwrap();
    fs::write(
        &path,
        text.replace(
            "content: 'class {{suffixed_name}} {}'",
            "source: linked.tmpl",
        ),
    )
    .unwrap();
    symlink(
        outside.path().join("pubspec.yaml"),
        path.parent().unwrap().join("linked.tmpl"),
    )
    .unwrap();
    assert!(load_pack(&path).is_err());
    let pack = catalog::bundled("sparrow.dart.class").unwrap();
    fs::create_dir(root.path().join(".git")).unwrap();
    assert!(
        plan_sibling(
            &context(root.path()),
            &root.path().join(".git"),
            &pack,
            "Counter"
        )
        .is_err()
    );
}
