use codesmith_core::CommandProposal;
use serde::Deserialize;
use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AgentOutput {
    Text(String),
    Command(CommandProposal),
}

#[derive(Debug, thiserror::Error)]
pub enum AgentError {
    #[error("empty agent output")]
    Empty,
}

#[derive(Debug, Deserialize)]
struct CommandProposalWire {
    command: String,
    cwd: PathBuf,
    reason: String,
}

pub fn parse_agent_output(input: &str) -> Result<AgentOutput, AgentError> {
    if input.trim().is_empty() {
        return Err(AgentError::Empty);
    }

    if let Some(proposal) = parse_command_proposal_wire(input.trim()) {
        return Ok(AgentOutput::Command(proposal));
    }

    for (index, _) in input.match_indices('{') {
        if let Some(proposal) = parse_first_command_proposal_from(&input[index..]) {
            return Ok(AgentOutput::Command(proposal));
        }
    }

    Ok(AgentOutput::Text(input.to_string()))
}

fn parse_first_command_proposal_from(input: &str) -> Option<CommandProposal> {
    let mut stream = serde_json::Deserializer::from_str(input).into_iter::<CommandProposalWire>();
    stream.next()?.ok().and_then(command_proposal_from_wire)
}

fn parse_command_proposal_wire(input: &str) -> Option<CommandProposal> {
    serde_json::from_str::<CommandProposalWire>(input)
        .ok()
        .and_then(command_proposal_from_wire)
}

fn command_proposal_from_wire(wire: CommandProposalWire) -> Option<CommandProposal> {
    if wire.command.trim().is_empty() {
        return None;
    }

    Some(CommandProposal::new(wire.command, wire.cwd, wire.reason))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_strict_command_proposal_json() {
        let parsed = parse_agent_output(
            r#"{"command":"echo hello","cwd":"/tmp","reason":"inspect greeting"}"#,
        )
        .expect("parse should not error");

        match parsed {
            AgentOutput::Command(proposal) => {
                assert_eq!(proposal.command, "echo hello");
                assert_eq!(proposal.cwd.to_string_lossy(), "/tmp");
                assert_eq!(proposal.reason, "inspect greeting");
                assert!(proposal.requires_approval);
            }
            AgentOutput::Text(_) => panic!("expected command proposal"),
        }
    }

    #[test]
    fn treats_malformed_json_as_text() {
        let input = r#"{"command":"echo hello", "#;
        let parsed = parse_agent_output(input).expect("malformed json should become text");
        assert_eq!(parsed, AgentOutput::Text(input.to_string()));
    }

    #[test]
    fn treats_non_proposal_json_as_text() {
        let input = r#"{"message":"hello"}"#;
        let parsed = parse_agent_output(input).expect("non proposal json should become text");
        assert_eq!(parsed, AgentOutput::Text(input.to_string()));
    }

    #[test]
    fn parses_command_proposal_embedded_after_explanation() {
        let input = r#"I will create the file first.

{"command":"printf ok","cwd":".","reason":"create a smoke file"}"#;
        let parsed = parse_agent_output(input).expect("embedded proposal should parse");

        match parsed {
            AgentOutput::Command(proposal) => {
                assert_eq!(proposal.command, "printf ok");
                assert_eq!(proposal.cwd.to_string_lossy(), ".");
                assert_eq!(proposal.reason, "create a smoke file");
            }
            AgentOutput::Text(_) => panic!("expected embedded command proposal"),
        }
    }
}
