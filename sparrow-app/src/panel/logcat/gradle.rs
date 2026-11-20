//! Ask AGP for final application IDs; never interpret arbitrary Gradle source as literals.
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeSet,
    fs,
    io::{Read, Seek, SeekFrom},
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::atomic::{AtomicBool, Ordering},
    time::{Duration, Instant},
};
const INIT: &str =
    include_str!("../../../../resources/logcat/gradle-model.init.gradle");
const LIMIT: u64 = 4 * 1024 * 1024;
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Model {
    pub schema: u32,
    pub variants: Vec<Variant>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Flavor {
    pub dimension: String,
    pub name: String,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Variant {
    pub module: String,
    pub name: String,
    pub build_type: String,
    pub flavor: String,
    #[serde(default)]
    pub flavors: Vec<Flavor>,
    pub application_id: String,
}
impl Variant {
    pub fn key(&self) -> String {
        format!("{}/{}", self.module, self.name)
    }
}
impl Model {
    pub fn parse(text: &str) -> Result<Self, String> {
        let model: Self = serde_json::from_str(text)
            .map_err(|e| format!("Invalid Gradle model: {e}"))?;
        if model.schema != 1 || model.variants.len() > 4096 {
            return Err("Unsupported Gradle variant model".into());
        }
        let mut seen = BTreeSet::new();
        for v in &model.variants {
            if !v.module.starts_with(':')
                || v.name.is_empty()
                || v.application_id.is_empty()
                || !v.application_id.split('.').all(|part| {
                    !part.is_empty()
                        && part
                            .bytes()
                            .all(|b| b.is_ascii_alphanumeric() || b == b'_')
                })
                || !seen.insert(v.key())
            {
                return Err("Invalid or duplicate Gradle application variant".into());
            }
        }
        Ok(model)
    }
    pub fn selected(&self, manual: &str, args: &[String]) -> Vec<&Variant> {
        if !manual.is_empty() {
            return self.variants.iter().filter(|v| v.key() == manual).collect();
        }
        let args =
            &args[..args.iter().position(|s| s == "--").unwrap_or(args.len())];
        for arg in args {
            let task = arg.rsplit(':').next().unwrap_or(arg);
            if let Some(name) =
                ["assemble", "install", "bundle"].iter().find_map(|prefix| {
                    task.strip_prefix(prefix).filter(|name| !name.is_empty())
                })
            {
                let module = arg
                    .rsplit_once(':')
                    .map(|(module, _)| module)
                    .filter(|s| !s.is_empty());
                return self
                    .variants
                    .iter()
                    .filter(|v| {
                        v.name.eq_ignore_ascii_case(name)
                            && module.is_none_or(|module| module == v.module)
                    })
                    .collect();
            }
        }
        let mut flavor = "";
        let mut build_type = "debug";
        for (i, arg) in args.iter().enumerate() {
            match arg.as_str() {
                "--flavor" => {
                    flavor = args.get(i + 1).map(String::as_str).unwrap_or("")
                }
                "--release" => build_type = "release",
                "--profile" => build_type = "profile",
                "--debug" => build_type = "debug",
                _ => {
                    if let Some(value) = arg.strip_prefix("--flavor=") {
                        flavor = value;
                    }
                }
            }
        }
        self.variants
            .iter()
            .filter(|v| v.build_type == build_type && v.flavor == flavor)
            .collect()
    }
    pub fn packages(&self, manual: &str, args: &[String]) -> Vec<String> {
        self.selected(manual, args)
            .into_iter()
            .map(|v| v.application_id.clone())
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect()
    }
}
pub fn build_root(workspace: &Path) -> Option<PathBuf> {
    [workspace.to_path_buf(), workspace.join("android")]
        .into_iter()
        .find(|p| {
            p.join("settings.gradle").is_file()
                || p.join("settings.gradle.kts").is_file()
        })
}
#[derive(Serialize, Deserialize)]
struct Cache {
    fingerprint: String,
    model: Model,
}
fn cache_path(workspace: &Path) -> PathBuf {
    workspace.join(".sparrow/cache/logcat-gradle.json")
}
fn fingerprint(root: &Path) -> Result<String, String> {
    fn visit(
        dir: &Path,
        depth: usize,
        files: &mut Vec<PathBuf>,
    ) -> std::io::Result<()> {
        if depth > 16 {
            return Ok(());
        }
        for entry in fs::read_dir(dir)? {
            let entry = entry?;
            let kind = entry.file_type()?;
            let path = entry.path();
            if kind.is_symlink() {
                continue;
            }
            if kind.is_dir() {
                if !matches!(
                    entry.file_name().to_str(),
                    Some(
                        "build"
                            | ".gradle"
                            | ".git"
                            | ".sparrow"
                            | ".dart_tool"
                            | "node_modules"
                    )
                ) {
                    visit(&path, depth + 1, files)?;
                }
            } else if matches!(
                entry.file_name().to_str(),
                Some("gradlew" | "gradlew.bat" | "gradle-wrapper.jar")
            ) || path.extension().is_some_and(|e| {
                matches!(
                    e.to_str(),
                    Some(
                        "gradle"
                            | "kts"
                            | "properties"
                            | "toml"
                            | "java"
                            | "kt"
                            | "groovy"
                    )
                )
            }) {
                files.push(path);
            }
        }
        Ok(())
    }
    let mut files = Vec::new();
    visit(root, 0, &mut files).map_err(|e| e.to_string())?;
    files.sort();
    let mut hash = Sha256::new();
    hash.update(root.to_string_lossy().as_bytes());
    hash.update(INIT.as_bytes());
    for path in files {
        hash.update(
            path.strip_prefix(root)
                .unwrap_or(&path)
                .to_string_lossy()
                .as_bytes(),
        );
        hash.update([0]);
        hash.update(fs::read(path).map_err(|e| e.to_string())?);
        hash.update([0]);
    }
    Ok(format!("{:x}", hash.finalize()))
}
pub fn cached(workspace: &Path) -> Option<Model> {
    let root = build_root(workspace)?;
    let cache: Cache =
        serde_json::from_str(&fs::read_to_string(cache_path(workspace)).ok()?)
            .ok()?;
    if cache.fingerprint != fingerprint(&root).ok()? {
        return None;
    }
    Model::parse(&serde_json::to_string(&cache.model).ok()?).ok()
}
fn stop(child: &mut Child) {
    #[cfg(unix)]
    {
        let _ = Command::new("kill")
            .args(["-KILL", "--", &format!("-{}", child.id())])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }
    #[cfg(windows)]
    {
        let _ = Command::new("taskkill")
            .args(["/PID", &child.id().to_string(), "/T", "/F"])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }
    let _ = child.kill();
    let _ = child.wait();
}
fn run(
    mut command: Command,
    output: &Path,
    log: &mut fs::File,
    cancel: &AtomicBool,
) -> Result<Model, String> {
    command
        .stdin(Stdio::null())
        .stdout(log.try_clone().map_err(|e| e.to_string())?)
        .stderr(log.try_clone().map_err(|e| e.to_string())?);
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
    }
    if cancel.load(Ordering::Acquire) {
        return Err("Gradle sync cancelled".into());
    }
    let mut child = command
        .spawn()
        .map_err(|e| format!("Could not start Gradle: {e}"))?;
    let start = Instant::now();
    loop {
        if cancel.load(Ordering::Acquire)
            || start.elapsed() > Duration::from_secs(180)
        {
            stop(&mut child);
            return Err(if cancel.load(Ordering::Acquire) {
                "Gradle sync cancelled"
            } else {
                "Gradle sync timed out after 3 minutes"
            }
            .into());
        }
        if log.metadata().is_ok_and(|m| m.len() > LIMIT) {
            stop(&mut child);
            return Err("Gradle sync output exceeded 4 MiB".into());
        }
        match child.try_wait() {
            Ok(Some(status)) => {
                if !status.success() {
                    log.seek(SeekFrom::End(
                        -(log.metadata().map_err(|e| e.to_string())?.len().min(12000)
                            as i64),
                    ))
                    .map_err(|e| e.to_string())?;
                    let mut tail = Vec::new();
                    let _ = log.read_to_end(&mut tail);
                    return Err(format!(
                        "Gradle sync failed: {}",
                        String::from_utf8_lossy(&tail).trim()
                    ));
                }
                if fs::metadata(output)
                    .map_err(|e| {
                        format!("Gradle did not produce a variant model: {e}")
                    })?
                    .len()
                    > LIMIT
                {
                    return Err("Gradle variant model exceeded 4 MiB".into());
                }
                return Model::parse(
                    &fs::read_to_string(output).map_err(|e| e.to_string())?,
                );
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(50)),
            Err(e) => {
                stop(&mut child);
                return Err(e.to_string());
            }
        }
    }
}
pub fn sync(workspace: &Path, cancel: &AtomicBool) -> Result<Model, String> {
    let root = build_root(workspace).ok_or(
        "No Android Gradle settings found in the workspace or android directory",
    )?;
    let before = fingerprint(&root)?;
    let temp = tempfile::tempdir().map_err(|e| e.to_string())?;
    let init = temp.path().join("model.init.gradle");
    let output = temp.path().join("model.json");
    fs::write(&init, INIT).map_err(|e| e.to_string())?;
    #[cfg(not(windows))]
    let mut command = {
        let wrapper = root.join("gradlew");
        if !wrapper.is_file() {
            return Err(
                "Gradle wrapper not found; add gradlew to the Android project"
                    .into(),
            );
        }
        let mut c = Command::new("sh");
        c.arg(wrapper);
        c
    };
    #[cfg(windows)]
    let mut command = {
        let wrapper = root.join("gradlew.bat");
        if !wrapper.is_file() {
            return Err("Gradle wrapper not found".into());
        }
        let mut c = Command::new("cmd");
        c.arg("/C").arg(wrapper);
        c
    };
    command
        .current_dir(&root)
        .args([
            "--console=plain",
            "--no-daemon",
            "--no-configuration-cache",
            "--no-configure-on-demand",
            "--init-script",
        ])
        .arg(init)
        .arg(format!("-Dsparrow.logcat.model={}", output.display()))
        .arg(":sparrowLogcatVariantModel");
    let mut log = tempfile::tempfile().map_err(|e| e.to_string())?;
    let model = run(command, &output, &mut log, cancel)?;
    if model.variants.is_empty() {
        return Err("No Android application variants found; library modules have no application ID".into());
    }
    if fingerprint(&root)? != before {
        return Err("Gradle configuration changed during sync; sync again".into());
    }
    if !cancel.load(Ordering::Acquire) {
        let path = cache_path(workspace);
        if let Some(parent) = path.parent() {
            let _ = fs::create_dir_all(parent);
        }
        let cache = Cache {
            fingerprint: before,
            model: model.clone(),
        };
        if let Ok(bytes) = serde_json::to_vec(&cache) {
            let tmp = path.with_extension("tmp");
            if fs::write(&tmp, bytes).is_ok() {
                let _ = fs::rename(tmp, path);
            }
        }
    }
    Ok(model)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn model() -> Model {
        Model {
            schema: 1,
            variants: vec![
                Variant {
                    module: ":app".into(),
                    name: "devDemoDebug".into(),
                    build_type: "debug".into(),
                    flavor: "devDemo".into(),
                    flavors: vec![
                        Flavor {
                            dimension: "environment".into(),
                            name: "dev".into(),
                        },
                        Flavor {
                            dimension: "edition".into(),
                            name: "demo".into(),
                        },
                    ],
                    application_id: "io.app.dev.demo.debug".into(),
                },
                Variant {
                    module: ":app".into(),
                    name: "prodFullRelease".into(),
                    build_type: "release".into(),
                    flavor: "prodFull".into(),
                    flavors: vec![],
                    application_id: "io.app".into(),
                },
                Variant {
                    module: ":app".into(),
                    name: "devDemoProfile".into(),
                    build_type: "profile".into(),
                    flavor: "devDemo".into(),
                    flavors: vec![],
                    application_id: "io.app.dev.demo.profile".into(),
                },
                Variant {
                    module: ":companion".into(),
                    name: "debug".into(),
                    build_type: "debug".into(),
                    flavor: "".into(),
                    flavors: vec![],
                    application_id: "io.companion.debug".into(),
                },
            ],
        }
    }
    #[test]
    fn follows_flavor_mode_and_explicit_variant_without_leaking_other_ids() {
        let m = model();
        let args = |values: &[&str]| {
            values.iter().map(|s| s.to_string()).collect::<Vec<_>>()
        };
        assert_eq!(
            m.packages("", &args(&["run", "--flavor", "devDemo"])),
            vec!["io.app.dev.demo.debug"]
        );
        assert_eq!(
            m.packages("", &args(&["run", "--flavor=devDemo", "--profile"])),
            vec!["io.app.dev.demo.profile"]
        );
        assert_eq!(
            m.packages("", &args(&["run", "--flavor", "prodFull", "--release"])),
            vec!["io.app"]
        );
        assert_eq!(
            m.packages(":app/prodFullRelease", &args(&["run", "--flavor=devDemo"])),
            vec!["io.app"]
        );
        assert!(m.packages(":app/deletedDebug", &[]).is_empty());
        assert_eq!(
            m.packages("", &args(&[":app:assembleProdFullRelease"])),
            vec!["io.app"]
        );
        assert_eq!(
            m.packages("", &args(&["run", "--", "--flavor=devDemo"])),
            vec!["io.companion.debug"]
        );
    }
    #[test]
    fn validates_model_and_flavor_dimensions() {
        let m = model();
        let parsed = Model::parse(&serde_json::to_string(&m).unwrap()).unwrap();
        assert_eq!(parsed.variants[0].flavors[1].dimension, "edition");
        assert!(Model::parse(r#"{"schema":2,"variants":[]}"#).is_err());
        let mut invalid = m.clone();
        invalid.variants[0].application_id = "${applicationId}".into();
        assert!(Model::parse(&serde_json::to_string(&invalid).unwrap()).is_err());
        let mut duplicate = m.clone();
        duplicate.variants.push(m.variants[0].clone());
        assert!(Model::parse(&serde_json::to_string(&duplicate).unwrap()).is_err());
    }
    #[test]
    fn cache_invalidates_on_gradle_catalog_and_build_logic_edits() {
        let dir = tempfile::tempdir().unwrap();
        let workspace = dir.path();
        let android = workspace.join("android");
        fs::create_dir_all(android.join("gradle")).unwrap();
        fs::write(android.join("settings.gradle.kts"), "include(\":app\")").unwrap();
        fs::write(android.join("gradle/libs.versions.toml"), "agp = '9.3.1'")
            .unwrap();
        let store = || {
            let cache = Cache {
                fingerprint: fingerprint(&android).unwrap(),
                model: model(),
            };
            fs::create_dir_all(cache_path(workspace).parent().unwrap()).unwrap();
            fs::write(cache_path(workspace), serde_json::to_vec(&cache).unwrap())
                .unwrap();
        };
        store();
        assert!(cached(workspace).is_some());
        fs::write(android.join("gradle/libs.versions.toml"), "agp = '9.5.1'")
            .unwrap();
        assert!(cached(workspace).is_none());
        store();
        fs::create_dir_all(android.join("build/generated")).unwrap();
        fs::write(android.join("build/generated/ignored.gradle"), "generated")
            .unwrap();
        assert!(cached(workspace).is_some());
        fs::write(android.join("settings.gradle.kts"), "include(\":other\")")
            .unwrap();
        assert!(cached(workspace).is_none());
    }
    #[cfg(unix)]
    #[test]
    fn sync_reads_wrapper_model_and_cancellation_reaps_its_process() {
        use std::sync::Arc;
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        fs::write(root.join("settings.gradle"), "").unwrap();
        let json = serde_json::to_string(&model()).unwrap();
        fs::write(root.join("gradlew"),format!("for arg in \"$@\"; do case \"$arg\" in -Dsparrow.logcat.model=*) out=\"${{arg#*=}}\";; esac; done\nprintf '%s' '{}' > \"$out\"\n",json)).unwrap();
        let cancel = AtomicBool::new(false);
        assert_eq!(sync(root, &cancel).unwrap(), model());
        assert_eq!(cached(root).unwrap(), model());
        let pidfile = root.join("worker.pid");
        fs::write(
            root.join("gradlew"),
            format!("echo $$ > '{}'\nexec sleep 10\n", pidfile.display()),
        )
        .unwrap();
        let cancel = Arc::new(AtomicBool::new(false));
        let worker_cancel = cancel.clone();
        let path = root.to_path_buf();
        let worker = std::thread::spawn(move || sync(&path, &worker_cancel));
        for _ in 0..100 {
            if pidfile.exists() {
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        let pid = fs::read_to_string(&pidfile).unwrap();
        cancel.store(true, Ordering::Release);
        assert!(worker.join().unwrap().unwrap_err().contains("cancelled"));
        assert!(
            !Command::new("kill")
                .args(["-0", pid.trim()])
                .stderr(Stdio::null())
                .status()
                .unwrap()
                .success()
        );
        assert!(cached(root).is_none());
    }
    #[test]
    #[ignore = "requires a real Android Gradle fixture in SPARROW_GRADLE_TEST_PROJECT"]
    fn reports_actual_agp_flavors_and_provider_overrides() {
        let root = PathBuf::from(
            std::env::var_os("SPARROW_GRADLE_TEST_PROJECT").expect("fixture path"),
        );
        let model = sync(&root, &AtomicBool::new(false)).unwrap();
        assert_eq!(model.variants.len(), 14);
        let id = |name: &str| {
            model
                .variants
                .iter()
                .find(|v| v.module == ":app" && v.name == name)
                .unwrap()
                .application_id
                .clone()
        };
        assert_eq!(id("devDemoDebug"), "com.android.dev.demo.debug");
        assert_eq!(id("devFullProfile"), "com.android.dev.profile");
        assert_eq!(id("prodFullRelease"), "com.android.systemui");
        assert_eq!(id("prodDemoRelease"), "com.android.settings");
        assert_eq!(
            model
                .variants
                .iter()
                .find(|v| v.module == ":companion" && v.name == "debug")
                .unwrap()
                .application_id,
            "org.sparrow.companion.test"
        );
        assert_eq!(cached(&root).unwrap(), model);
    }
}
