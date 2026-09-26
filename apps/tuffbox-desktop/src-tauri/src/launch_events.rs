use serde::Serialize;
use tuffbox_core::launch_error::LaunchErrorInfo;
use tuffbox_core::process::RunningProcess;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LaunchProgressEvent {
    pub instance_id: String,
    pub profile_id: String,
    pub phase: String,
    pub message: String,
    pub percent: Option<u32>,
}

impl LaunchProgressEvent {
    pub fn new(
        instance_id: impl Into<String>,
        profile_id: impl Into<String>,
        phase: impl Into<String>,
        message: impl Into<String>,
        percent: Option<u32>,
    ) -> Self {
        Self {
            instance_id: instance_id.into(),
            profile_id: profile_id.into(),
            phase: phase.into(),
            message: message.into(),
            percent,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LaunchCrashEvent {
    pub instance_id: String,
    pub profile_id: String,
    #[serde(flatten)]
    pub error: LaunchErrorInfo,
}

impl LaunchCrashEvent {
    pub fn new(
        instance_id: impl Into<String>,
        profile_id: impl Into<String>,
        error: LaunchErrorInfo,
    ) -> Self {
        Self {
            instance_id: instance_id.into(),
            profile_id: profile_id.into(),
            error,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProcessStartedEvent {
    pub id: String,
    pub profile_id: String,
    pub pid: u32,
    pub started_at: u64,
}

impl From<&RunningProcess> for ProcessStartedEvent {
    fn from(process: &RunningProcess) -> Self {
        Self {
            id: process.id.clone(),
            profile_id: process.profile_id.clone(),
            pid: process.pid,
            started_at: process.started_at,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProcessExitedEvent {
    pub id: String,
    pub profile_id: String,
    pub pid: u32,
    pub code: Option<i32>,
}

impl ProcessExitedEvent {
    pub fn new(
        id: impl Into<String>,
        profile_id: impl Into<String>,
        pid: u32,
        code: Option<i32>,
    ) -> Self {
        Self {
            id: id.into(),
            profile_id: profile_id.into(),
            pid,
            code,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{LaunchCrashEvent, LaunchProgressEvent, ProcessExitedEvent};
    use tuffbox_core::launch_error::{LaunchErrorInfo, LaunchErrorKind};

    #[test]
    fn progress_payload_identifies_the_launch_session() {
        let payload = serde_json::to_value(LaunchProgressEvent::new(
            r"C:\packs\alpha\tuffbox.json",
            "client",
            "install",
            "Installing Minecraft…",
            Some(55),
        ))
        .unwrap();

        assert_eq!(payload["instanceId"], r"C:\packs\alpha\tuffbox.json");
        assert_eq!(payload["profileId"], "client");
        assert_eq!(payload["phase"], "install");
        assert_eq!(payload["percent"], 55);
    }

    #[test]
    fn crash_payload_keeps_error_fields_and_session_owner() {
        let error = LaunchErrorInfo::new(LaunchErrorKind::LaunchCrash, "JVM exited");
        let payload = serde_json::to_value(LaunchCrashEvent::new(
            "C:/packs/alpha/tuffbox.json",
            "client",
            error,
        ))
        .unwrap();

        assert_eq!(payload["instanceId"], "C:/packs/alpha/tuffbox.json");
        assert_eq!(payload["profileId"], "client");
        assert_eq!(payload["kind"], "launch_crash");
        assert_eq!(payload["message"], "JVM exited");
    }

    #[test]
    fn exit_payload_includes_pid_to_reject_stale_events() {
        let payload = serde_json::to_value(ProcessExitedEvent::new(
            "C:/packs/alpha/tuffbox.json",
            "client",
            42,
            Some(1),
        ))
        .unwrap();

        assert_eq!(payload["id"], "C:/packs/alpha/tuffbox.json");
        assert_eq!(payload["profileId"], "client");
        assert_eq!(payload["pid"], 42);
        assert_eq!(payload["code"], 1);
    }
}
