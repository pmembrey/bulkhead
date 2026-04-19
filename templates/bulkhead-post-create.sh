#!/usr/bin/env bash
set -euo pipefail

selected_agents="${BULKHEAD_SELECTED_AGENTS:-}"
readonly NVM_VERSION="v0.40.4"

if [[ -z "${selected_agents}" ]]; then
  exit 0
fi

export PATH="${HOME}/.local/bin:${PATH}"
export NVM_DIR="${HOME}/.nvm"
mkdir -p "${HOME}/.local/bin"

has_agent() {
  case ",${selected_agents}," in
    *",$1,"*) return 0 ;;
    *) return 1 ;;
  esac
}

run_privileged() {
  if [[ "$(id -u)" -eq 0 ]]; then
    "$@"
  else
    sudo "$@"
  fi
}

ensure_writable_dir() {
  local dir="$1"
  local owner

  if [[ ! -d "${dir}" ]]; then
    mkdir -p "${dir}" 2>/dev/null || run_privileged mkdir -p "${dir}"
  fi

  if [[ -w "${dir}" ]]; then
    return 0
  fi

  owner="$(id -u):$(id -g)"
  run_privileged chown -R "${owner}" "${dir}"

  if [[ ! -w "${dir}" ]]; then
    echo "bulkhead: ${dir} is still not writable after fixing ownership." >&2
    return 1
  fi
}

ensure_npm() {
  if command -v npm >/dev/null 2>&1; then
    return 0
  fi

  echo "bulkhead: npm is not available; rebuild after enabling the Node devcontainer feature." >&2
  return 1
}

ensure_local_binary_link() {
  local source_path="$1"
  local command_name="$2"
  local local_bin

  local_bin="${HOME}/.local/bin/${command_name}"

  if [[ ! -x "${source_path}" ]]; then
    echo "bulkhead: expected executable ${source_path} for ${command_name}, but it was not found or is not executable." >&2
    return 1
  fi

  ln -sf "${source_path}" "${local_bin}"

  if [[ ! -x "${local_bin}" ]]; then
    echo "bulkhead: failed to link ${command_name} into ${local_bin}." >&2
    return 1
  fi
}

ensure_global_binary_on_path() {
  local command_name="$1"
  local global_bin

  global_bin="$(npm prefix -g)/bin/${command_name}"
  ensure_local_binary_link "${global_bin}" "${command_name}"
}

install_npm_agent() {
  local package_name="$1"
  local command_name="$2"
  local global_bin

  ensure_npm
  global_bin="$(npm prefix -g)/bin/${command_name}"

  if [[ ! -x "${global_bin}" ]]; then
    env \
      NPM_CONFIG_AUDIT=false \
      NPM_CONFIG_FUND=false \
      NPM_CONFIG_IGNORE_SCRIPTS=false \
      npm install -g "${package_name}"
  fi

  ensure_global_binary_on_path "${command_name}"
}

ensure_nvm_shell_init() {
  local bashrc marker

  bashrc="${HOME}/.bashrc"
  marker="# >>> bulkhead nvm >>>"

  if grep -Fq "${marker}" "${bashrc}" 2>/dev/null; then
    return 0
  fi

  cat >>"${bashrc}" <<'EOF'
# >>> bulkhead nvm >>>
export NVM_DIR="$HOME/.nvm"
[ -s "$NVM_DIR/nvm.sh" ] && \. "$NVM_DIR/nvm.sh"
[ -s "$NVM_DIR/bash_completion" ] && \. "$NVM_DIR/bash_completion"
# <<< bulkhead nvm <<<
EOF
}

load_nvm() {
  if [[ ! -s "${NVM_DIR}/nvm.sh" ]]; then
    echo "bulkhead: nvm was expected at ${NVM_DIR}/nvm.sh but is missing." >&2
    return 1
  fi

  # shellcheck disable=SC1090
  . "${NVM_DIR}/nvm.sh"
}

install_nvm() {
  if [[ -s "${NVM_DIR}/nvm.sh" ]]; then
    ensure_nvm_shell_init
    load_nvm
    return 0
  fi

  curl -o- "https://raw.githubusercontent.com/nvm-sh/nvm/${NVM_VERSION}/install.sh" | PROFILE=/dev/null bash
  ensure_nvm_shell_init
  load_nvm
}

link_nvm_runtime_binaries() {
  local node_bin_dir

  node_bin_dir="$(dirname "$(nvm which current)")"
  ensure_local_binary_link "${node_bin_dir}/node" "node"
  ensure_local_binary_link "${node_bin_dir}/npm" "npm"
  ensure_local_binary_link "${node_bin_dir}/npx" "npx"

  if [[ -x "${node_bin_dir}/corepack" ]]; then
    ensure_local_binary_link "${node_bin_dir}/corepack" "corepack"
  fi
}

ensure_latest_nvm_node() {
  install_nvm
  nvm install node --latest-npm
  nvm alias default node >/dev/null
  nvm use default >/dev/null
  link_nvm_runtime_binaries
}

configure_claude() {
  local claude_dir settings_file tmp_file

  claude_dir="${CLAUDE_CONFIG_DIR:-${HOME}/.claude}"
  settings_file="${claude_dir}/settings.json"
  ensure_writable_dir "${claude_dir}"

  tmp_file="$(mktemp)"
  if [[ -f "${settings_file}" ]] && jq \
    '.permissions = (.permissions // {}) | .permissions.defaultMode = "bypassPermissions"' \
    "${settings_file}" >"${tmp_file}" 2>/dev/null; then
    if mv "${tmp_file}" "${settings_file}"; then
      return 0
    fi

    rm -f "${tmp_file}"
    return 1
  fi

  rm -f "${tmp_file}"
  cat >"${settings_file}" <<'EOF'
{
  "permissions": {
    "defaultMode": "bypassPermissions"
  }
}
EOF
}

bootstrap_claude_auth() {
  local token claude_json_dir claude_json tmp_file out_file err_file status

  token="${CLAUDE_CODE_OAUTH_TOKEN:-}"
  if [[ -z "${token}" ]]; then
    return 0
  fi

  # Trail of Bits documented this Claude quirk in claude-code-devcontainer:
  # when CLAUDE_CONFIG_DIR is set, Claude writes .claude.json inside that
  # directory instead of under $HOME, so we seed onboarding state there.
  claude_json_dir="${CLAUDE_CONFIG_DIR:-${HOME}}"
  claude_json="${claude_json_dir}/.claude.json"

  out_file="$(mktemp)"
  err_file="$(mktemp)"

  if timeout 30s claude -p ok >"${out_file}" 2>"${err_file}"; then
    :
  else
    status=$?
    case "${status}" in
      124)
        echo "bulkhead: claude auth bootstrap timed out after seeding config; continuing." >&2
        ;;
      *)
        if [[ -s "${err_file}" ]]; then
          echo "bulkhead: claude auth bootstrap returned a non-zero exit: $(tr '\n' ' ' <"${err_file}")" >&2
        else
          echo "bulkhead: claude auth bootstrap returned a non-zero exit." >&2
        fi
        ;;
    esac
  fi

  rm -f "${out_file}" "${err_file}"

  if [[ ! -f "${claude_json}" ]]; then
    echo "bulkhead: claude auth bootstrap did not create ${claude_json}; onboarding bypass skipped." >&2
    return 0
  fi

  tmp_file="$(mktemp)"
  if jq '.hasCompletedOnboarding = true' "${claude_json}" >"${tmp_file}" 2>/dev/null; then
    if mv "${tmp_file}" "${claude_json}"; then
      return 0
    fi

    rm -f "${tmp_file}"
    return 1
  fi

  rm -f "${tmp_file}"
  echo "bulkhead: ${claude_json} was not valid JSON after auth bootstrap; onboarding bypass skipped." >&2
}

if has_agent "pi"; then
  mkdir -p "${HOME}/.pi"
  ensure_latest_nvm_node
  install_npm_agent "@mariozechner/pi-coding-agent" "pi"
fi

if has_agent "claude"; then
  ensure_writable_dir "${CLAUDE_CONFIG_DIR:-${HOME}/.claude}"
  install_npm_agent "@anthropic-ai/claude-code" "claude"
  bootstrap_claude_auth
  configure_claude
fi

if has_agent "codex"; then
  ensure_writable_dir "${HOME}/.codex"
  install_npm_agent "@openai/codex" "codex"
fi
