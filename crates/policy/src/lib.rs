use codesmith_core::{CommandProposal, PolicyDecision, RiskLevel};
use std::path::Path;

pub fn evaluate(proposal: &CommandProposal, workspace: &Path) -> PolicyDecision {
    if !proposal.cwd.starts_with(workspace) {
        return PolicyDecision {
            allowed: false,
            requires_approval: true,
            risk_level: RiskLevel::Blocked,
            reason: format!(
                "working directory is outside configured workspace: {}",
                proposal.cwd.display()
            ),
        };
    }

    let command = proposal.command.to_lowercase();
    let destructive_patterns = [
        "rm -rf",
        "sudo ",
        "chmod -r",
        "chown -r",
        "mkfs",
        "diskutil erase",
        "format ",
        "security find-generic-password",
        "cat ~/.ssh",
        "cat /etc/passwd",
        "curl ",
        "wget ",
        "nc ",
        "netcat ",
        "scp ",
        "rsync ",
    ];

    if destructive_patterns
        .iter()
        .any(|pattern| command.contains(pattern))
    {
        return PolicyDecision {
            allowed: false,
            requires_approval: true,
            risk_level: RiskLevel::Blocked,
            reason: "command matches destructive, privileged, credential, or exfiltration pattern"
                .to_string(),
        };
    }

    PolicyDecision {
        allowed: true,
        requires_approval: true,
        risk_level: RiskLevel::Low,
        reason: "command is allowed after explicit approval".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn proposal(command: &str, cwd: &str) -> codesmith_core::CommandProposal {
        codesmith_core::CommandProposal::new(command, PathBuf::from(cwd), "test")
    }

    #[test]
    fn safe_command_is_allowed_but_requires_approval() {
        let workspace = PathBuf::from("/tmp/project");
        let decision = evaluate(&proposal("echo hello", "/tmp/project"), &workspace);

        assert!(decision.allowed);
        assert!(decision.requires_approval);
        assert_eq!(decision.risk_level, codesmith_core::RiskLevel::Low);
    }

    #[test]
    fn blocks_destructive_commands() {
        let workspace = PathBuf::from("/tmp/project");
        let decision = evaluate(&proposal("rm -rf /tmp/project", "/tmp/project"), &workspace);

        assert!(!decision.allowed);
        assert_eq!(decision.risk_level, codesmith_core::RiskLevel::Blocked);
        assert!(decision.reason.contains("destructive"));
    }

    #[test]
    fn blocks_commands_outside_workspace() {
        let workspace = PathBuf::from("/tmp/project");
        let decision = evaluate(&proposal("echo hello", "/tmp/other"), &workspace);

        assert!(!decision.allowed);
        assert!(decision.reason.contains("outside"));
    }
}
