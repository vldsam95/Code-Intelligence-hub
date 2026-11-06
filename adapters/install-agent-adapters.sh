#!/bin/sh
set -eu

clients="${1:-codex,claude,hermes}"
agent_user="${2:-${CODEINTEL_AGENT_USER:-${SUDO_USER:-$(id -un)}}}"
binary="${CODEINTEL_BINARY:-/usr/local/bin/codeintel}"
skill_source="${CODEINTEL_SKILL_SOURCE:-/usr/share/code-intelligence/SKILL.md}"
skill_source_root="$(dirname -- "$skill_source")"
case "$agent_user" in *[!A-Za-z0-9._-]*|'') echo "Invalid Agent user." >&2; exit 2 ;; esac
agent_home="$(getent passwd "$agent_user" | cut -d: -f6)"
[ -n "$agent_home" ] || { echo "Agent user does not exist: $agent_user" >&2; exit 2; }
skill_target="$agent_home/.agents/skills/code-intelligence"

mkdir -p "$skill_target"
if [ -f "$skill_source" ]; then
  cp "$skill_source" "$skill_target/SKILL.md"
  if [ -d "$skill_source_root/references" ]; then
    mkdir -p "$skill_target/references"
    for reference in "$skill_source_root"/references/*.md; do
      [ -f "$reference" ] || continue
      cp "$reference" "$skill_target/references/"
    done
  fi
  chmod 0644 "$skill_target/SKILL.md"
  chown -R "$agent_user" "$agent_home/.agents"
fi

find_client() {
  client_name="$1"
  if [ "$agent_user" = "$(id -un)" ]; then
    command -v "$client_name" 2>/dev/null || true
  else
    runuser -u "$agent_user" -- env HOME="$agent_home" sh -lc "command -v $client_name" 2>/dev/null || true
  fi
}

as_agent() {
  if [ "$agent_user" = "$(id -un)" ]; then
    HOME="$agent_home" "$@"
  else
    runuser -u "$agent_user" -- env HOME="$agent_home" "$@"
  fi
}

has_client() {
  case ",$clients," in
    *",$1,"*) return 0 ;;
    *) return 1 ;;
  esac
}

if has_client codex; then
  client_path="$(find_client codex)"
  if [ -n "$client_path" ] && [ -x "$client_path" ]; then
    if as_agent "$client_path" mcp get code-intelligence >/dev/null 2>&1; then
      as_agent "$client_path" mcp remove code-intelligence >/dev/null
    fi
    as_agent "$client_path" mcp add code-intelligence -- "$binary" mcp serve --stdio >/dev/null
    echo "Codex adapter: installed"
  else
    echo "Codex adapter: client not installed; rerun this adapter after installing Codex" >&2
  fi
fi

if has_client claude; then
  client_path="$(find_client claude)"
  if [ -n "$client_path" ] && [ -x "$client_path" ]; then
    as_agent "$client_path" mcp remove --scope user code-intelligence >/dev/null 2>&1 || true
    as_agent "$client_path" mcp add --scope user --transport stdio code-intelligence -- \
      "$binary" mcp serve --stdio >/dev/null
    echo "Claude Code adapter: installed"
  else
    echo "Claude Code adapter: client not installed; rerun this adapter after installing Claude Code" >&2
  fi
fi

if has_client hermes; then
  client_path="$(find_client hermes)"
  if [ -n "$client_path" ] && [ -x "$client_path" ]; then
    as_agent "$client_path" mcp remove code-intelligence >/dev/null 2>&1 || true
    as_agent "$client_path" mcp add code-intelligence --command "$binary" --args mcp serve --stdio >/dev/null
    echo "Hermes adapter: installed"
  else
    echo "Hermes adapter: client not installed; rerun this adapter after installing Hermes" >&2
  fi
fi

echo "$clients" > /etc/code-intelligence/clients.requested
chmod 0644 /etc/code-intelligence/clients.requested
