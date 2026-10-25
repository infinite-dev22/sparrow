use std::{collections::HashMap, fmt::Display, path::PathBuf};

use serde::{Deserialize, Serialize};

use crate::{debug::SparrowBreakpoint, main_split::SplitInfo, panel::data::PanelInfo};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Hash)]
pub struct SshHost {
    pub user: Option<String>,
    pub host: String,
    pub port: Option<usize>,
}

impl SshHost {
    pub fn from_string(s: &str) -> Self {
        let mut whole_splits = s.split(':');
        let splits = whole_splits
            .next()
            .unwrap()
            .split('@')
            .collect::<Vec<&str>>();
        let mut splits = splits.iter().rev();
        let host = splits.next().unwrap().to_string();
        let user = splits.next().map(|s| s.to_string());
        let port = whole_splits.next().and_then(|s| s.parse::<usize>().ok());
        Self { user, host, port }
    }

    pub fn user_host(&self) -> String {
        if let Some(user) = self.user.as_ref() {
            format!("{user}@{}", self.host)
        } else {
            self.host.clone()
        }
    }
}

impl Display for SshHost {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if let Some(user) = self.user.as_ref() {
            write!(f, "{user}@")?;
        }
        write!(f, "{}", self.host)?;
        if let Some(port) = self.port {
            write!(f, ":{port}")?;
        }
        Ok(())
    }
}

#[cfg(windows)]
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Hash)]
pub struct WslHost {
    pub host: String,
}

#[cfg(windows)]
impl Display for WslHost {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.host)?;
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum SparrowWorkspaceType {
    Local,
    RemoteSSH(SshHost),
    #[cfg(windows)]
    RemoteWSL(WslHost),
}

impl SparrowWorkspaceType {
    pub fn is_local(&self) -> bool {
        matches!(self, SparrowWorkspaceType::Local)
    }

    pub fn is_remote(&self) -> bool {
        use SparrowWorkspaceType::*;

        #[cfg(not(windows))]
        return matches!(self, RemoteSSH(_));

        #[cfg(windows)]
        return matches!(self, RemoteSSH(_) | RemoteWSL(_));
    }
}

impl std::fmt::Display for SparrowWorkspaceType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SparrowWorkspaceType::Local => f.write_str("Local"),
            SparrowWorkspaceType::RemoteSSH(remote) => {
                write!(f, "ssh://{remote}")
            }
            #[cfg(windows)]
            SparrowWorkspaceType::RemoteWSL(remote) => {
                write!(f, "{remote} (WSL)")
            }
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SparrowWorkspace {
    pub kind: SparrowWorkspaceType,
    pub path: Option<PathBuf>,
    pub last_open: u64,
}

impl SparrowWorkspace {
    pub fn display(&self) -> Option<String> {
        let path = self.path.as_ref()?;
        let path = path
            .file_name()
            .unwrap_or(path.as_os_str())
            .to_string_lossy()
            .to_string();
        let remote = match &self.kind {
            SparrowWorkspaceType::Local => String::new(),
            SparrowWorkspaceType::RemoteSSH(remote) => {
                format!(" [SSH: {}]", remote.host)
            }
            #[cfg(windows)]
            SparrowWorkspaceType::RemoteWSL(remote) => {
                format!(" [WSL: {}]", remote.host)
            }
        };
        Some(format!("{path}{remote}"))
    }
}

impl Default for SparrowWorkspace {
    fn default() -> Self {
        Self {
            kind: SparrowWorkspaceType::Local,
            path: None,
            last_open: 0,
        }
    }
}

impl std::fmt::Display for SparrowWorkspace {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}:{}",
            self.kind,
            self.path.as_ref().and_then(|p| p.to_str()).unwrap_or("")
        )
    }
}

#[derive(Clone, Serialize, Deserialize)]
pub struct WorkspaceInfo {
    pub split: SplitInfo,
    pub panel: PanelInfo,
    pub breakpoints: HashMap<PathBuf, Vec<SparrowBreakpoint>>,
}
