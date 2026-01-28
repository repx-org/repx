#!/usr/bin/env bash

# ==============================================================================
# Generic Nix Impure Build ACL Manager
#
# This script manages Filesystem ACLs to grant a Nix builder user temporary,
# minimal access to one or more directories within a user's home directory.
#
# It automatically sets traversal-only permissions on parent directories and
# full access on the specified target directories.
#
# ==============================================================================

readonly NIX_BUILD_GROUP="nixbld"

readonly C_GREEN='\033[0;32m'
readonly C_RED='\033[0;31m'
readonly C_YELLOW='\033[1;33m'
readonly C_BLUE='\033[0;34m'
readonly C_NC='\033[0m'

set -e
set -u
set -o pipefail

usage() {
  cat <<EOF
Usage: ${0} <command> <dir1> [dir2...]

A generic tool to manage ACLs for impure Nix builds.

Commands:
  ${C_GREEN}set${C_NC}      Sets the required ACLs for the Nix builder on the provided directories.
           Requires at least one directory path as an argument.

  ${C_RED}clean${C_NC}    Removes all ACLs from the provided directories and their parents.
           Requires at least one directory path as an argument.

  ${C_BLUE}check${C_NC}    Displays the current ACLs on the specified directories and their parents.
           Requires at least one directory path as an argument.

  ${C_YELLOW}audit${C_NC}    Searches your entire home directory for any file/folder with an ACL.
           (Does not require directory arguments).

  ${C_BLUE}help${C_NC}     Shows this message.
EOF
}

log() {
  echo -e "${1}[*] ${2}${C_NC}"
}

check_dependencies() {
  local missing_deps=0
  for cmd in setfacl getfacl realpath find dirname; do
    if ! command -v "$cmd" &>/dev/null; then
      log "$C_RED" "Error: Required command '$cmd' is not found in your PATH."
      missing_deps=1
    fi
  done
  if [ "$missing_deps" -eq 1 ]; then
    log "$C_RED" "Please install the 'acl' and 'coreutils' packages and try again."
    exit 1
  fi
}

_get_all_relevant_paths() {
  local all_paths=()
  for dir in "$@"; do
    if [ ! -d "$dir" ]; then
      log "$C_RED" "Error: Directory not found: '${dir}'"
      exit 1
    fi

    local current_path
    current_path=$(realpath "$dir")

    if [[ "$current_path" != "$HOME"* ]]; then
      log "$C_RED" "Security Error: Path '${current_path}' is outside of your home directory (${HOME}). Aborting."
      exit 1
    fi

    all_paths+=("$current_path")
    while [[ "$current_path" != "$HOME" && "$current_path" != "/" ]]; do
      current_path=$(dirname "$current_path")
      all_paths+=("$current_path")
    done
  done
  printf "%s\n" "${all_paths[@]}" | sort -u
}

do_set_acls() {
  log "$C_GREEN" "Applying ACLs for Nix builder group '${NIX_BUILD_GROUP}'..."
  local target_dirs=("$@")
  local all_paths
  all_paths=$(_get_all_relevant_paths "${target_dirs[@]}")

  while IFS= read -r path; do
    is_target=false
    for target in "${target_dirs[@]}"; do
      if [[ "$(realpath "$target")" == "$path" ]]; then
        is_target=true
        break
      fi
    done

    if [ "$is_target" = true ]; then
      log "$C_GREEN" "  Granting READ/WRITE on: ${path}"
      setfacl -m "g:${NIX_BUILD_GROUP}:rwx" "$path"
      setfacl -d -m "g:${NIX_BUILD_GROUP}:rwx" "$path"
      find "$path" -user "$USER" -exec setfacl -m "g:${NIX_BUILD_GROUP}:rwx" {} +
    else
      log "$C_GREEN" "  Granting TRAVERSE on:  ${path}"
      setfacl -m "g:${NIX_BUILD_GROUP}:--x" "$path"
    fi
  done <<<"$all_paths"

  log "$C_GREEN" "ACLs successfully set."
}

do_clean_acls() {
  log "$C_RED" "Cleaning up ACLs..."
  local relevant_paths
  relevant_paths=$(_get_all_relevant_paths "$@")

  while IFS= read -r path; do
    log "$C_RED" "  Cleaning ACLs from: ${path}"
    setfacl -b "$path"
  done <<<"$relevant_paths"

  log "$C_RED" "Cleanup complete. Permissions have been restored to default."
}

do_check_acls() {
  log "$C_BLUE" "Checking ACL status for relevant directories..."
  local relevant_paths
  relevant_paths=$(_get_all_relevant_paths "$@")

  while IFS= read -r path; do
    echo -e "\n${C_YELLOW}--- ${path} ---${C_NC}"
    getfacl "$path" | grep --color=always -E "^(user::|group::|other::|mask::|group:${NIX_BUILD_GROUP}:|$)" || true
  done <<<"$relevant_paths"
}

do_audit_acls() {
  log "$C_YELLOW" "Searching for all files/directories with ACLs in ${HOME}..."
  local find_results
  find_results=$(find "${HOME}" -acl -print 2>/dev/null || true)

  if [ -n "$find_results" ]; then
    echo "$find_results"
    log "$C_YELLOW" "Audit complete. The files/directories listed above have extended ACLs."
  else
    log "$C_GREEN" "Audit complete. No ACLs found in your home directory."
  fi
}

main() {
  check_dependencies

  if [ $# -eq 0 ]; then
    usage
    exit 1
  fi

  local command="${1}"
  shift

  case "$command" in
  set | clean | check)
    if [ $# -eq 0 ]; then
      log "$C_RED" "Error: The '${command}' command requires at least one directory path."
      usage
      exit 1
    fi
    "do_${command}_acls" "$@"
    ;;
  audit)
    do_audit_acls
    ;;
  help | -h | --help)
    usage
    ;;
  *)
    log "$C_RED" "Error: Unknown command '${command}'"
    usage
    exit 1
    ;;
  esac
}

main "$@"
