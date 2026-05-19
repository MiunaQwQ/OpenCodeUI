use std::sync::atomic::{AtomicBool, Ordering};

use tokio::sync::Mutex;

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct ServiceProcess {
    pub child_pid: Option<u32>,
}

/// 跟踪我们是否启动了 opencode serve 进程
pub struct ServiceState {
    /// App 自己持有的后端进程状态必须经过此 mutex 串行化，避免并发命令覆盖彼此的 PID。
    pub process: Mutex<ServiceProcess>,
    /// 是否由我们启动（用于关闭时判断是否需要询问）
    pub we_started: AtomicBool,
}

impl ServiceState {
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
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::Ordering;

    use super::ServiceState;

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
}
