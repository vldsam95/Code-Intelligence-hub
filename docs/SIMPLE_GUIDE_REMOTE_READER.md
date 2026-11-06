# Practical guide: let a remote agent read a project or prepare a presentation

## Goal

Give a remote agent context, architecture, health and history without sending full source or confusing its role with an agent that changes code.

## Your steps

1. Open the project page in the Hub and go to **Project Access & Agent Setup**.
2. Click Reveal/Copy and insert Project Access into the prepared prompt. The same token serves all authorized agents.
3. Send the document to the remote agent. It receives context and structured live analysis, but no remote shell.
4. To disconnect all agents holding the old credential, click Rotate or Revoke.

## Complete prompt

The project page generates the full prompt because it must contain the specific project ID, Hub endpoint and token. The canonical template is `REMOTE_PROJECT_READER_PROMPT.md`.
