use anyhow::Result;
use codesmith_core::{CommandProposal, CommandRun, CommandStatus, RunnerEvent};
use std::process::Stdio;
use std::time::Duration;
use tokio::io::AsyncReadExt;
use tokio::process::Command;
use tokio::sync::mpsc::UnboundedSender;
use tokio::time;

pub async fn run_approved_command(
    proposal: CommandProposal,
    timeout: Duration,
) -> Result<CommandRun> {
    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
    run_approved_command_streaming(proposal, timeout, tx).await
}

pub async fn run_approved_command_streaming(
    proposal: CommandProposal,
    timeout: Duration,
    events: UnboundedSender<RunnerEvent>,
) -> Result<CommandRun> {
    let mut child = Command::new("/bin/sh")
        .arg("-c")
        .arg(&proposal.command)
        .current_dir(&proposal.cwd)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;

    let mut stdout = child.stdout.take().expect("stdout should be piped");
    let mut stderr = child.stderr.take().expect("stderr should be piped");

    let stdout_events = events.clone();
    let stdout_task = tokio::spawn(async move {
        let mut buf = String::new();
        stdout.read_to_string(&mut buf).await.map(|_| {
            if !buf.is_empty() {
                let _ = stdout_events.send(RunnerEvent::Stdout(buf.clone()));
            }
            buf
        })
    });
    let stderr_events = events.clone();
    let stderr_task = tokio::spawn(async move {
        let mut buf = String::new();
        stderr.read_to_string(&mut buf).await.map(|_| {
            if !buf.is_empty() {
                let _ = stderr_events.send(RunnerEvent::Stderr(buf.clone()));
            }
            buf
        })
    });

    let status = match time::timeout(timeout, child.wait()).await {
        Ok(status) => status?,
        Err(_) => {
            let _ = child.kill().await;
            let stdout = stdout_task.await??;
            let stderr = stderr_task.await??;
            return Ok(CommandRun::new(
                proposal,
                CommandStatus::TimedOut,
                stdout,
                stderr,
                None,
            ));
        }
    };

    let stdout = stdout_task.await??;
    let stderr = stderr_task.await??;
    let exit_code = status.code();
    let run_status = if status.success() {
        CommandStatus::Succeeded
    } else {
        CommandStatus::Failed
    };
    let _ = events.send(RunnerEvent::Finished(run_status));

    Ok(CommandRun::new(
        proposal, run_status, stdout, stderr, exit_code,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use codesmith_core::CommandProposal;
    use std::path::PathBuf;
    use std::time::Duration;

    #[tokio::test]
    async fn approved_echo_streams_stdout() {
        let proposal = CommandProposal::new("echo hello", PathBuf::from("."), "test");
        let result = run_approved_command(proposal, Duration::from_secs(5))
            .await
            .expect("command should run");

        assert_eq!(result.exit_code, Some(0));
        assert!(result.stdout.contains("hello"));
        assert!(result.stderr.is_empty());
    }

    #[tokio::test]
    async fn command_times_out() {
        let proposal = CommandProposal::new("sleep 2", PathBuf::from("."), "test");
        let result = run_approved_command(proposal, Duration::from_millis(100))
            .await
            .expect("timeout should return a run");

        assert_eq!(result.status, codesmith_core::CommandStatus::TimedOut);
        assert_eq!(result.exit_code, None);
    }

    #[tokio::test]
    async fn streams_stdout_events_before_finished_event() {
        let proposal = CommandProposal::new("printf hello", PathBuf::from("."), "test");
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let result = run_approved_command_streaming(proposal, Duration::from_secs(5), tx)
            .await
            .expect("command should run");
        let mut events = Vec::new();
        while let Ok(event) = rx.try_recv() {
            events.push(event);
        }

        assert_eq!(result.status, codesmith_core::CommandStatus::Succeeded);
        assert!(events.iter().any(|event| {
            matches!(event, codesmith_core::RunnerEvent::Stdout(chunk) if chunk.contains("hello"))
        }));
        assert!(matches!(
            events.last(),
            Some(codesmith_core::RunnerEvent::Finished(
                codesmith_core::CommandStatus::Succeeded
            ))
        ));
    }
}
