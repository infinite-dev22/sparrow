//! Run the pinned analyzer outside the UI thread and merge its proposed edits.
use super::engine::{Change, Context, Plan};
use serde::Deserialize;
use serde_json::json;
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::Mutex,
    time::{Duration, Instant},
};

const MANIFEST: &str =
    include_str!("../../resources/scaffold_analyzer/pubspec.yaml");
const LOCK: &str = include_str!("../../resources/scaffold_analyzer/pubspec.lock");
const SCRIPT: &str =
    include_str!("../../resources/scaffold_analyzer/bin/registration.dart");
static SETUP: Mutex<()> = Mutex::new(());

#[derive(Deserialize)]
struct Response {
    changes: Vec<Proposed>,
    notes: Vec<String>,
    error: Option<String>,
}
#[derive(Deserialize)]
struct Proposed {
    path: PathBuf,
    before: String,
    after: String,
}

/// Bounded child execution; neither helper setup nor analysis blocks the UI.
fn run(
    mut command: Command,
    input: Option<&str>,
    timeout: Duration,
) -> Result<String, String> {
    let mut stdin = tempfile::NamedTempFile::new().map_err(|e| e.to_string())?;
    if let Some(input) = input {
        stdin
            .write_all(input.as_bytes())
            .map_err(|e| e.to_string())?;
    }
    let stdout = tempfile::NamedTempFile::new().map_err(|e| e.to_string())?;
    let stderr = tempfile::NamedTempFile::new().map_err(|e| e.to_string())?;
    command
        .stdin(Stdio::from(
            fs::File::open(stdin.path()).map_err(|e| e.to_string())?,
        ))
        .stdout(Stdio::from(stdout.reopen().map_err(|e| e.to_string())?))
        .stderr(Stdio::from(stderr.reopen().map_err(|e| e.to_string())?));
    let mut child = command
        .spawn()
        .map_err(|e| format!("Could not launch scaffold analyzer: {e}"))?;
    let start = Instant::now();
    loop {
        if stdout
            .as_file()
            .metadata()
            .map(|m| m.len())
            .unwrap_or(u64::MAX)
            > 16 * 1024 * 1024
            || stderr
                .as_file()
                .metadata()
                .map(|m| m.len())
                .unwrap_or(u64::MAX)
                > 2 * 1024 * 1024
        {
            let _ = child.kill();
            let _ = child.wait();
            return Err("Scaffold analyzer output exceeded its limit".into());
        }
        match child.try_wait() {
            Ok(Some(status)) => {
                if stdout
                    .as_file()
                    .metadata()
                    .map_err(|e| e.to_string())?
                    .len()
                    > 16 * 1024 * 1024
                    || stderr
                        .as_file()
                        .metadata()
                        .map_err(|e| e.to_string())?
                        .len()
                        > 2 * 1024 * 1024
                {
                    return Err("Scaffold analyzer output exceeded its limit".into());
                }
                let output =
                    fs::read_to_string(stdout.path()).map_err(|e| e.to_string())?;
                if status.success() {
                    return Ok(output);
                }
                if let Ok(response) =
                    serde_json::from_str::<serde_json::Value>(&output)
                {
                    if let Some(error) =
                        response.get("error").and_then(|v| v.as_str())
                    {
                        return Err(error.to_string());
                    }
                }
                return Err(format!(
                    "Scaffold analyzer failed: {} {}",
                    output,
                    fs::read_to_string(stderr.path()).unwrap_or_default()
                ));
            }
            Ok(None) if start.elapsed() < timeout => {
                std::thread::sleep(Duration::from_millis(30))
            }
            result => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(match result {Err(e)=>e.to_string(),_=>"Scaffold analysis timed out. Check package resolution and preview again.".into()});
            }
        }
    }
}

#[cfg(test)]
mod process_tests {
    use super::*;
    fn child(mode: &str) -> Command {
        let mut c = Command::new(std::env::current_exe().unwrap());
        c.args([
            "--exact",
            "scaffold::registration::process_tests::helper_child",
            "--ignored",
            "--nocapture",
        ])
        .env("SPARROW_TEST_HELPER_MODE", mode);
        c
    }
    #[test]
    #[ignore = "subprocess fixture invoked only by process safety tests"]
    fn helper_child() {
        match std::env::var("SPARROW_TEST_HELPER_MODE").unwrap().as_str() {
            "crash" => panic!("fixture crashed"),
            "output" => {
                std::io::stdout()
                    .write_all(&vec![b'x'; 17 * 1024 * 1024])
                    .unwrap();
                std::thread::sleep(Duration::from_secs(2));
            }
            _ => std::thread::sleep(Duration::from_secs(2)),
        }
    }
    #[test]
    fn timed_out_helper_is_killed_and_reaped() {
        assert!(
            run(child("sleep"), None, Duration::from_millis(40))
                .unwrap_err()
                .contains("timed out")
        );
    }
    #[test]
    fn crashed_helper_is_a_reported_error() {
        assert!(
            run(child("crash"), None, Duration::from_secs(10))
                .unwrap_err()
                .contains("fixture crashed")
        );
    }
    #[test]
    fn output_limit_stops_a_still_running_helper() {
        assert!(
            run(child("output"), None, Duration::from_secs(10))
                .unwrap_err()
                .contains("exceeded its limit")
        );
    }
    #[test]
    fn incomplete_or_malformed_success_responses_are_rejected() {
        for output in ["{}", "not json", r#"{"notes":[]}"#, r#"{"changes":[]}"#] {
            assert!(
                serde_json::from_str::<Response>(output).is_err(),
                "{output}"
            );
        }
        assert!(
            serde_json::from_str::<Response>(r#"{"changes":[],"notes":[]}"#).is_ok()
        );
    }
}
fn helper(dart: &Path) -> Result<PathBuf, String> {
    use sha2::{Digest, Sha256};
    let _lock = SETUP.lock().map_err(|e| e.to_string())?;
    let stamp = dart.metadata().and_then(|m| m.modified()).ok();
    let key = format!(
        "{:x}",
        Sha256::digest(format!(
            "{MANIFEST}{LOCK}{SCRIPT}{}{stamp:?}",
            dart.display()
        ))
    );
    let base = sparrow_core::directory::Directory::cache_directory()
        .ok_or("No IDE cache directory is available")?
        .join("scaffold-analyzer")
        .join(key);
    fs::create_dir_all(base.join("bin")).map_err(|e| e.to_string())?;
    for (name, content) in [
        ("pubspec.yaml", MANIFEST),
        ("pubspec.lock", LOCK),
        ("bin/registration.dart", SCRIPT),
    ] {
        let path = base.join(name);
        if path.is_symlink() {
            return Err("Scaffold analyzer cache contains a symlink".into());
        }
        if fs::read_to_string(&path).ok().as_deref() != Some(content) {
            fs::write(path, content).map_err(|e| e.to_string())?;
        }
    }
    if !base.join(".dart_tool/package_config.json").is_file() {
        let command = |offline| {
            let mut c = Command::new(dart);
            c.current_dir(&base)
                .args(["pub", "get", "--enforce-lockfile"]);
            if offline {
                c.arg("--offline");
            }
            c
        };
        if run(command(true), None, Duration::from_secs(60)).is_err() {
            run(command(false),None,Duration::from_secs(120)).map_err(|e|format!("Could not prepare the bundled analyzer (first use needs its pinned packages). {e}"))?;
        }
    }
    Ok(base)
}
fn collect(
    path: &Path,
    files: &mut Vec<(PathBuf, String)>,
    bytes: &mut usize,
) -> Result<(), String> {
    if !path.exists() || path.is_symlink() {
        return Ok(());
    }
    if path.is_dir() {
        let mut entries = fs::read_dir(path)
            .map_err(|e| e.to_string())?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())?;
        entries.sort_by_key(|e| e.path());
        for entry in entries {
            collect(&entry.path(), files, bytes)?;
        }
    } else if path.extension().is_some_and(|e| e == "dart")
        && !path
            .file_name()
            .unwrap()
            .to_string_lossy()
            .ends_with(".g.dart")
        && !path
            .file_name()
            .unwrap()
            .to_string_lossy()
            .ends_with(".freezed.dart")
    {
        let size = path.metadata().map_err(|e| e.to_string())?.len();
        if size > 2 * 1024 * 1024
            || *bytes + size as usize > 8 * 1024 * 1024
            || files.len() >= 2000
        {
            return Err("Registration scope exceeds 2,000 files / 8 MB. Narrow the target package.".into());
        }
        let source = fs::read_to_string(path).map_err(|e| e.to_string())?;
        *bytes += source.len();
        files.push((path.to_path_buf(), source));
    }
    Ok(())
}
pub fn augment(mut plan: Plan, ctx: &Context, name: &str) -> Result<Plan, String> {
    let dart = crate::dart::sdk::find_dart_binary(Some(&ctx.root))
        .ok_or("Configure a Dart/Flutter SDK before automatic registration")?;
    let (filename, _) = super::render(&super::template("name", "", None, ""), name)?;
    let snake = filename.trim_end_matches(".dart");
    let screen_dir = if ctx.layout == "layer-first" {
        format!("presentation/{snake}")
    } else {
        format!(
            "{}/{snake}/presentation",
            ctx.feature_root.trim_start_matches("lib/")
        )
    };
    let repository_dir = if ctx.layout == "layer-first" {
        format!("data/{snake}")
    } else {
        format!(
            "{}/{snake}/data",
            ctx.feature_root.trim_start_matches("lib/")
        )
    };
    let mut sources = vec![];
    collect(&ctx.root.join("lib"), &mut sources, &mut 0)?;
    let config = ctx.root.join(".dart_tool/package_config.json");
    let config_before = fs::read_to_string(&config).ok();
    if !sources.is_empty() && config_before.is_none() {
        return Err(
            "Run Pub Get in the target package before automatic registration."
                .into(),
        );
    }
    if sources.is_empty() {
        plan.notes.push("No existing Dart source to register into. Generated route factory is available.".into());
        return Ok(plan);
    }
    let pubspec: serde_yaml::Value = serde_yaml::from_str(
        &fs::read_to_string(ctx.root.join("pubspec.yaml"))
            .map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    let provider_available = pubspec
        .get("dependencies")
        .and_then(|d| d.get("provider"))
        .is_some();
    let request = json!({"root":ctx.root,"name":name,"snake":snake,"screen_uri":format!("package:{}/{screen_dir}/{snake}_screen.dart",ctx.package),"state_uri":format!("package:{}/{screen_dir}/{snake}_view_model.dart",ctx.package),"repository_uri":format!("package:{}/{repository_dir}/{snake}_repository.dart",ctx.package),"settings":ctx.registration,"provider_available":provider_available,
        "sources":sources.iter().map(|(path,content)|json!({"path":path,"content":content})).collect::<Vec<_>>(),
        "overlays":plan.changes.iter().filter(|c|c.path.extension().is_some_and(|e|e=="dart")).filter_map(|c|c.after.as_ref().map(|content|json!({"path":c.path,"content":content}))).collect::<Vec<_>>()});
    let base = helper(&dart)?;
    let mut command = Command::new(&dart);
    command
        .current_dir(&base)
        .arg(format!(
            "--packages={}",
            base.join(".dart_tool/package_config.json").display()
        ))
        .arg(base.join("bin/registration.dart"));
    let output = run(
        command,
        Some(&request.to_string()),
        Duration::from_secs(120),
    )?;
    let response: Response = serde_json::from_str(&output)
        .map_err(|e| format!("Invalid scaffold analyzer response: {e}"))?;
    if let Some(error) = response.error {
        return Err(error);
    }
    let changes = response
        .changes
        .into_iter()
        .map(|c| Change {
            path: c.path,
            before: Some(c.before),
            after: Some(c.after),
        })
        .collect();
    let mut observed = sources
        .into_iter()
        .map(|(p, s)| (p, Some(s)))
        .collect::<Vec<_>>();
    observed.push((config, config_before));
    plan.add_registration(changes, observed, response.notes)?;
    Ok(plan)
}

#[cfg(test)]
mod sdk_tests {
    use super::*;
    #[test]
    #[ignore = "requires SPARROW_REGISTRATION_EXPORT_ROOT and SPARROW_TEST_FLUTTER_SDK for isolated real-package fixtures"]
    fn real_flutter_registration_preview_apply_idempotency_and_undo() {
        let root = PathBuf::from(
            std::env::var_os("SPARROW_REGISTRATION_EXPORT_ROOT")
                .expect("fixture root"),
        );
        let sdk = PathBuf::from(
            std::env::var_os("SPARROW_TEST_FLUTTER_SDK").expect("Flutter SDK root"),
        );
        run_suite(
            &root,
            &sdk,
            "6.1.5+1",
            "18.1.4",
            &[
                "material",
                "material_provider",
                "material_wrap",
                "go_provider",
            ],
            true,
        );
    }
    fn run_suite(
        root: &Path,
        sdk: &Path,
        provider_version: &str,
        router_version: &str,
        modes: &[&str],
        offline: bool,
    ) {
        for &mode in modes {
            let package = root.join(mode);
            assert!(!package.exists(), "use a fresh fixture root");
            fs::create_dir_all(package.join("lib")).unwrap();
            fs::create_dir_all(package.join(".sparrow")).unwrap();
            fs::write(
                package.join(".sparrow/settings.toml"),
                format!(
                    "[core]\nflutter-sdk-path = {:?}\n",
                    sdk.display().to_string()
                ),
            )
            .unwrap();
            let provider = mode != "material";
            fs::write(package.join("pubspec.yaml"),format!("name: registration_example\nenvironment:\n  sdk: '>=3.10.0 <4.0.0'\ndependencies:\n  flutter:\n    sdk: flutter\n{}{}dev_dependencies:\n  flutter_test:\n    sdk: flutter\n",if provider{format!("  provider: {provider_version}\n")}else{String::new()},if mode=="go_provider"{format!("  go_router: {router_version}\n")}else{String::new()})).unwrap();
            let flutter = crate::dart::sdk::find_flutter_binary(Some(&package))
                .expect("Flutter executable");
            let mut resolve = Command::new(&flutter);
            resolve.current_dir(&package).args(["pub", "get"]);
            if offline {
                resolve.arg("--offline");
            }
            let output = resolve.output().unwrap();
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
            let main = if mode == "go_provider" {
                fs::write(package.join("lib/router.dart"),"import 'package:flutter/material.dart';\nimport 'package:go_router/go_router.dart';\nfinal router=GoRouter(routes:[GoRoute(path:'/',builder:(_,state)=>const Scaffold(body:Text('Home')))]);\n").unwrap();
                "import 'package:flutter/material.dart';\nimport 'router.dart';\nvoid main()=>runApp(buildApp());\nWidget buildApp()=>MaterialApp.router(routerConfig:router);\n".to_string()
            } else if mode == "material_provider" {
                "import 'package:flutter/material.dart';\nimport 'package:provider/provider.dart';\nvoid main()=>runApp(buildApp());\nWidget buildApp()=>MultiProvider(providers:[],child:MaterialApp(home:const Scaffold(body:Text('Home'))));\n".into()
            } else {
                "import 'package:flutter/material.dart';\nvoid main()=>runApp(buildApp());\nWidget buildApp()=>MaterialApp(home:const Scaffold(body:Text('Home')));\n".into()
            };
            fs::write(package.join("lib/main.dart"), &main).unwrap();
            let ctx = super::super::engine::context(&package, &package).unwrap();
            let pack = super::super::feature::pack();
            let plan = augment(
                super::super::engine::plan_pack(&ctx, &pack, "Orders").unwrap(),
                &ctx,
                "Orders",
            )
            .unwrap();
            assert_eq!(
                fs::read_to_string(package.join("lib/main.dart")).unwrap(),
                main,
                "preview changed source"
            );
            assert!(
                plan.changes
                    .iter()
                    .any(|c| c.path == package.join("lib/main.dart")),
                "missing main registration for {mode}"
            );
            let applied = plan.apply().unwrap();
            let repeated = augment(
                super::super::engine::plan_pack(&ctx, &pack, "Orders").unwrap(),
                &ctx,
                "Orders",
            )
            .unwrap();
            assert!(
                repeated.changes.is_empty(),
                "registration is not idempotent for {mode}"
            );
            applied.undo().unwrap();
            assert_eq!(
                fs::read_to_string(package.join("lib/main.dart")).unwrap(),
                main
            );
            assert!(!package.join("lib/features/orders").exists());
            plan.apply().unwrap();
            let test = format!(
                "import 'package:flutter/material.dart';\nimport 'package:flutter_test/flutter_test.dart';\nimport 'package:registration_example/main.dart' as app;\nimport 'package:registration_example/features/orders/presentation/orders_screen.dart';\n{}{}\nvoid main() {{testWidgets('registered route opens and consumes registered state',(tester) async {{\nawait tester.pumpWidget(app.buildApp());await tester.pumpAndSettle();\n{}\nawait tester.pumpAndSettle();expect(find.text('No items yet'),findsOneWidget);\n{}\n}});}}\n",
                if mode == "go_provider" {
                    "import 'package:registration_example/router.dart' as navigation;\n"
                } else {
                    ""
                },
                if provider {
                    "import 'package:provider/provider.dart';\nimport 'package:registration_example/features/orders/presentation/orders_view_model.dart';\n"
                } else {
                    ""
                },
                if mode == "go_provider" {
                    "navigation.router.go('/orders');"
                } else {
                    "Navigator.of(tester.element(find.text('Home'))).pushNamed('/orders');"
                },
                if provider {
                    "final screen=tester.widget<OrdersScreen>(find.byType(OrdersScreen));\nexpect(screen.viewModel,same(Provider.of<OrdersViewModel>(tester.element(find.byType(OrdersScreen)),listen:false)));"
                } else {
                    "expect(tester.widget<OrdersScreen>(find.byType(OrdersScreen)).viewModel,isNull);"
                }
            );
            fs::write(package.join("test/registration_test.dart"), test).unwrap();
            let mut command = Command::new(&flutter);
            command.current_dir(&package).arg("test");
            let output = run(command, None, Duration::from_secs(180)).unwrap();
            fs::write(package.join("flutter-tests.log"), output).unwrap();
            println!("PASS resolved AST preview/apply/idempotency/undo: {mode}");
        }
    }
    #[test]
    #[ignore = "requires SPARROW_MATRIX_SDKS JSON array and a fresh SPARROW_REGISTRATION_EXPORT_ROOT"]
    fn compatibility_matrix() {
        let sdks: Vec<PathBuf> = serde_json::from_str(
            &std::env::var("SPARROW_MATRIX_SDKS").expect("SDK roots JSON array"),
        )
        .unwrap();
        assert!(!sdks.is_empty());
        let root = PathBuf::from(
            std::env::var_os("SPARROW_REGISTRATION_EXPORT_ROOT")
                .expect("fresh report root"),
        );
        assert!(!root.exists());
        fs::create_dir_all(&root).unwrap();
        let matrix: serde_json::Value = serde_json::from_str(include_str!(
            "../../resources/scaffold_analyzer/compatibility.json"
        ))
        .unwrap();
        let mut report = vec![];
        for (index, sdk) in sdks.iter().enumerate() {
            let flutter = sdk.join("bin").join(if cfg!(windows) {
                "flutter.bat"
            } else {
                "flutter"
            });
            let mut command = Command::new(flutter);
            command.args(["--version", "--machine"]);
            let version =
                run(command, None, Duration::from_secs(120)).and_then(|s| {
                    serde_json::from_str::<serde_json::Value>(&s)
                        .map_err(|e| e.to_string())
                });
            for case in matrix["packages"].as_array().unwrap() {
                let id = case["id"].as_str().unwrap();
                let package_root = root.join(format!("sdk-{index}-{id}"));
                let result = std::panic::catch_unwind(|| {
                    assert!(version.is_ok(), "{version:?}");
                    run_suite(
                        &package_root,
                        sdk,
                        case["provider"].as_str().unwrap(),
                        case["go_router"].as_str().unwrap(),
                        &["go_provider"],
                        std::env::var_os("SPARROW_MATRIX_ONLINE").is_none(),
                    );
                });
                let failure = result.err().map(|e| {
                    e.downcast_ref::<String>()
                        .cloned()
                        .or_else(|| e.downcast_ref::<&str>().map(|s| s.to_string()))
                        .unwrap_or_else(|| "test panicked".into())
                });
                report.push(json!({"sdk":sdk,"sdk_version":version.as_ref().ok(),"packages":case,"status":if failure.is_none(){"passed"}else{"failed"},"failure":failure,"fixture":package_root}));
                fs::write(
                    root.join("report.json"),
                    serde_json::to_vec_pretty(&json!({"schema":1,"results":report}))
                        .unwrap(),
                )
                .unwrap();
            }
        }
        assert!(
            report.iter().all(|r| r["status"] == "passed"),
            "Compatibility failures: inspect {}",
            root.join("report.json").display()
        );
    }
}
