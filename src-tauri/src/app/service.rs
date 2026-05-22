use serde::{Deserialize, Serialize};
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
    sync::atomic::{AtomicBool, Ordering},
};

use tokio::sync::Mutex;

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct ServiceProcess {
    pub child_pid: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
struct OwnedServiceMarker {
    pid: u32,
    started_at: String,
}

/// 跟踪我们是否启动了 opencode serve 进程
pub struct ServiceState {
    /// App 自己持有的后端进程状态必须经过此 mutex 串行化，避免并发命令覆盖彼此的 PID。
    pub process: Mutex<ServiceProcess>,
    /// 是否由我们启动（用于关闭时判断是否需要询问）
    pub we_started: AtomicBool,
    ownership_marker_path: Option<PathBuf>,
}

impl ServiceState {
    pub fn new(ownership_marker_path: PathBuf) -> Self {
        let restored_pid = Self::restore_owned_pid(&ownership_marker_path);

        Self {
            process: Mutex::new(ServiceProcess {
                child_pid: restored_pid,
            }),
            we_started: AtomicBool::new(restored_pid.is_some()),
            ownership_marker_path: Some(ownership_marker_path),
        }
    }

    fn restore_owned_pid(ownership_marker_path: &Path) -> Option<u32> {
        let marker = match Self::load_ownership_marker(ownership_marker_path) {
            Some(marker) => marker,
            None => return None,
        };

        if current_process_started_at(marker.pid).as_deref() == Some(marker.started_at.as_str()) {
            return Some(marker.pid);
        }

        Self::clear_ownership_marker_file(ownership_marker_path);
        None
    }

    fn load_ownership_marker(ownership_marker_path: &Path) -> Option<OwnedServiceMarker> {
        let marker_json = fs::read_to_string(ownership_marker_path).ok()?;
        serde_json::from_str(&marker_json).ok()
    }

    fn clear_ownership_marker_file(ownership_marker_path: &Path) {
        if let Err(error) = fs::remove_file(ownership_marker_path) {
            if error.kind() != std::io::ErrorKind::NotFound {
                log::warn!(
                    "Failed to clear service ownership marker '{}': {}",
                    ownership_marker_path.display(),
                    error
                );
            }
        }
    }

    fn persist_ownership_marker(&self, marker: &OwnedServiceMarker) -> Result<(), String> {
        let Some(ownership_marker_path) = self.ownership_marker_path.as_ref() else {
            return Ok(());
        };

        if let Some(parent) = ownership_marker_path.parent() {
            fs::create_dir_all(parent).map_err(|error| {
                format!(
                    "Failed to create service ownership directory '{}': {}",
                    parent.display(),
                    error
                )
            })?;
        }

        let marker_json = serde_json::to_vec(marker)
            .map_err(|error| format!("Failed to serialize service ownership marker: {}", error))?;

        fs::write(ownership_marker_path, marker_json).map_err(|error| {
            format!(
                "Failed to persist service ownership marker '{}': {}",
                ownership_marker_path.display(),
                error
            )
        })
    }

    fn clear_ownership_marker(&self) {
        if let Some(ownership_marker_path) = self.ownership_marker_path.as_ref() {
            Self::clear_ownership_marker_file(ownership_marker_path);
        }
    }

    pub fn register_spawned_pid_locked(
        &self,
        process: &mut ServiceProcess,
        pid: u32,
    ) -> Result<(), String> {
        let marker = if self.ownership_marker_path.is_some() {
            Some(OwnedServiceMarker {
                pid,
                started_at: current_process_started_at(pid).ok_or_else(|| {
                    format!("Failed to capture spawned process start time for PID {}", pid)
                })?,
            })
        } else {
            None
        };

        process.child_pid = Some(pid);

        if let Some(marker) = marker.as_ref() {
            if let Err(error) = self.persist_ownership_marker(marker) {
                process.child_pid = None;
                self.we_started.store(false, Ordering::SeqCst);
                return Err(error);
            }
        }

        self.we_started.store(true, Ordering::SeqCst);
        Ok(())
    }

    #[cfg(test)]
    pub async fn register_spawned_pid(&self, pid: u32) -> Result<(), String> {
        let mut process = self.process.lock().await;
        self.register_spawned_pid_locked(&mut process, pid)
    }

    #[cfg(test)]
    pub async fn set_child_pid(&self, pid: u32) {
        self.process.lock().await.child_pid = Some(pid);
    }

    #[cfg(test)]
    pub async fn take_child_pid(&self) -> Option<u32> {
        self.process.lock().await.child_pid.take()
    }

    pub async fn take_owned_pid_for_shutdown(&self) -> Option<u32> {
        let mut process = self.process.lock().await;
        let pid = process.child_pid.take();
        self.we_started.store(false, Ordering::SeqCst);
        self.clear_ownership_marker();
        drop(process);
        pid
    }

    #[cfg(test)]
    pub async fn clear_child_pid(&self) {
        self.process.lock().await.child_pid = None;
    }
}

impl Default for ServiceState {
    fn default() -> Self {
        Self {
            process: Mutex::new(ServiceProcess::default()),
            we_started: AtomicBool::new(false),
            ownership_marker_path: None,
        }
    }
}

fn current_process_started_at(pid: u32) -> Option<String> {
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;

        const CREATE_NO_WINDOW: u32 = 0x08000000;

        let mut command = Command::new("powershell");
        command.creation_flags(CREATE_NO_WINDOW);

        command
            .args([
                "-NoLogo",
                "-NoProfile",
                "-NonInteractive",
                "-Command",
                &format!(
                    "$process = Get-Process -Id {} -ErrorAction SilentlyContinue; if ($process) {{ $process.StartTime.ToUniversalTime().ToString('o') }}",
                    pid
                ),
            ])
            .output()
            .ok()
            .filter(|output| output.status.success())
            .map(|output| {
                String::from_utf8_lossy(&output.stdout).trim().to_string()
            })
            .filter(|started_at| !started_at.is_empty())
    }

    #[cfg(not(target_os = "windows"))]
    {
        Command::new("ps")
            .args(["-p", &pid.to_string(), "-o", "lstart="])
            .output()
            .ok()
            .filter(|output| output.status.success())
            .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_string())
            .filter(|started_at| !started_at.is_empty())
    }
}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        path::PathBuf,
        sync::atomic::Ordering,
        time::{SystemTime, UNIX_EPOCH},
    };

    use super::ServiceState;

    fn unique_marker_path(test_name: &str) -> PathBuf {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock should be after epoch")
            .as_nanos();

        std::env::temp_dir().join(format!(
            "opencodeui-{test_name}-{}-{timestamp}.json",
            std::process::id()
        ))
    }

    #[tokio::test]
    async fn service_state_default() {
        let state = ServiceState::default();

        assert_eq!(state.process.lock().await.child_pid, None);
        assert!(!state.we_started.load(Ordering::SeqCst));
    }

    #[tokio::test]
    async fn service_state_take_pid() {
        let state = ServiceState::default();

        state.set_child_pid(1234).await;
        assert_eq!(state.process.lock().await.child_pid, Some(1234));

        assert_eq!(state.take_child_pid().await, Some(1234));
        assert_eq!(state.process.lock().await.child_pid, None);

        state.set_child_pid(5678).await;
        state.clear_child_pid().await;
        assert_eq!(state.take_child_pid().await, None);
        assert!(!state.we_started.load(Ordering::SeqCst));
    }

    #[tokio::test]
    async fn service_state_restores_owned_pid_from_marker() {
        let marker_path = unique_marker_path("restore-owned-pid");
        let pid = std::process::id();
        let state = ServiceState::new(marker_path.clone());

        state
            .register_spawned_pid(pid)
            .await
            .expect("marker should be written");

        let state = ServiceState::new(marker_path.clone());

        assert_eq!(state.process.lock().await.child_pid, Some(pid));
        assert!(state.we_started.load(Ordering::SeqCst));

        let _ = fs::remove_file(marker_path);
    }

    #[tokio::test]
    async fn service_state_discards_stale_marker() {
        let marker_path = unique_marker_path("discard-stale-marker");
        let pid = std::process::id();

        fs::write(
            &marker_path,
            format!("{{\"pid\":{pid},\"started_at\":\"stale-start\"}}"),
        )
            .expect("marker should be written");

        let state = ServiceState::new(marker_path.clone());

        assert_eq!(state.process.lock().await.child_pid, None);
        assert!(!state.we_started.load(Ordering::SeqCst));
        assert!(!marker_path.exists());
    }

    #[tokio::test]
    async fn register_spawned_pid_persists_marker() {
        let marker_path = unique_marker_path("persist-marker");
        let pid = std::process::id();
        let state = ServiceState::new(marker_path.clone());

        state
            .register_spawned_pid(pid)
            .await
            .expect("marker persistence should succeed");

        assert_eq!(state.process.lock().await.child_pid, Some(pid));
        assert!(state.we_started.load(Ordering::SeqCst));
        let marker = fs::read_to_string(&marker_path).expect("marker should exist");
        assert!(marker.contains(&format!("\"pid\":{pid}")));
        assert!(marker.contains("\"started_at\":\""));

        let taken_pid = state.take_owned_pid_for_shutdown().await;
        assert_eq!(taken_pid, Some(pid));
        assert!(!marker_path.exists());
    }
}
