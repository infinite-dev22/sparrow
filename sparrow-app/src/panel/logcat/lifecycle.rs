use super::model::Entry;
use std::collections::HashMap;
#[derive(Default)]
pub struct Tracker {
    serial: String,
    previous: Option<HashMap<u32, (String, String)>>,
}
impl Tracker {
    pub fn observe(
        &mut self,
        serial: &str,
        processes: &HashMap<u32, (String, String)>,
        mine: &[String],
    ) -> Vec<Entry> {
        if self.serial != serial {
            self.serial = serial.into();
            self.previous = None;
        }
        // An unavailable/unsupported ps result is not proof that applications died.
        if processes.is_empty() {
            return Vec::new();
        }
        let current = processes
            .iter()
            .filter(|(_, (package, _))| {
                if mine.is_empty() {
                    package.contains('.')
                } else {
                    mine.contains(package)
                }
            })
            .map(|(pid, names)| (*pid, names.clone()))
            .collect::<HashMap<_, _>>();
        let mut entries = Vec::new();
        if let Some(previous) = &self.previous {
            let mut ended = previous
                .iter()
                .filter(|(pid, names)| current.get(pid) != Some(*names))
                .collect::<Vec<_>>();
            ended.sort_by_key(|(pid, _)| **pid);
            for (pid, (package, process)) in ended {
                entries.push(marker(*pid, package, process, false));
            }
            let mut started = current
                .iter()
                .filter(|(pid, names)| previous.get(pid) != Some(*names))
                .collect::<Vec<_>>();
            started.sort_by_key(|(pid, _)| **pid);
            for (pid, (package, process)) in started {
                entries.push(marker(*pid, package, process, true));
            }
        }
        self.previous = Some(current);
        entries
    }
}
pub fn marker(pid: u32, package: &str, process: &str, started: bool) -> Entry {
    let now = chrono::Local::now();
    let message = format!(
        "--------- PROCESS {} ({pid}) for package {package} ---------",
        if started { "STARTED" } else { "ENDED" }
    );
    Entry {
        pid,
        package: package.into(),
        process: process.into(),
        tag: "Sparrow.Process".into(),
        level: 'I',
        timestamp: now.format("%Y-%m-%d %H:%M:%S%.3f").to_string(),
        time: now.timestamp(),
        raw: message.clone(),
        message,
        marker: true,
        ..Default::default()
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn marks_restart_and_pid_reuse_without_false_initial_or_disconnect_events() {
        let mut t = Tracker::default();
        let mine = vec!["io.app".into()];
        let p = |pid, name: &str| {
            HashMap::from([
                (1, ("system".into(), "init".into())),
                (pid, ("io.app".into(), name.into())),
            ])
        };
        assert!(t.observe("a", &p(10, "io.app"), &mine).is_empty());
        assert!(t.observe("a", &HashMap::new(), &mine).is_empty());
        let e = t.observe("a", &p(11, "io.app"), &mine);
        assert_eq!(e.len(), 2);
        assert!(e[0].message.contains("ENDED (10)"));
        assert!(e[1].message.contains("STARTED (11)"));
        let e = t.observe("a", &p(11, "io.app:worker"), &mine);
        assert_eq!(e.len(), 2);
        assert!(t.observe("b", &p(12, "io.app"), &mine).is_empty());
    }
}
