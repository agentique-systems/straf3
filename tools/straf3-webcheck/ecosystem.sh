#!/usr/bin/env bash
# Stand the whole straf3 web ecosystem up on one origin, and take it down again.
#
#   tools/straf3-webcheck/ecosystem.sh up       build the client, start both
#                                               service binaries and the site
#   tools/straf3-webcheck/ecosystem.sh down     stop everything, leave nothing
#   tools/straf3-webcheck/ecosystem.sh status   what is listening, and what is not
#   tools/straf3-webcheck/ecosystem.sh logs     tail every process's log
#
# The topology is the wave contract's §A, and it is the whole point: a browser
# only ever talks to http://localhost:8787.
#
#   http://localhost:8787          the site — web/dev/serve.mjs
#     /v1/*                        proxied to the records service
#     /client/*                    crates/straf3-game/web/pkg
#     /assets/maps/*               assets/maps
#     everything else              web/site
#   127.0.0.1:8788                 the records service, never addressed by a page
#
# ── the credential ─────────────────────────────────────────────────────────
#
# Every secret lives in the gitignored `.env` at the repository root and is
# read from there at start-up. This script contains no credential, prints no
# credential, and puts none in a log line or a process title: the service is
# handed `DATABASE_URL` through its environment, which is where sqlx reads it
# from anyway. `status` prints which variables are SET, never their values.
#
# `.env` is gitignored, which means it does not travel between git worktrees.
# If you are in one and it is missing, copy it from the main checkout; the
# error message says so.
#
# ── partial bring-up is a supported outcome ────────────────────────────────
#
# A piece that does not exist yet is reported and skipped, not fatal. Standing
# up the site without the records service is a genuinely useful state — it is
# the `no_records_service` 503 the site has to render as *unanswerable* rather
# than as an empty board — so this script produces it deliberately rather than
# refusing to start.

set -uo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$root" || exit 1

run_dir="target/straf3-ecosystem"     # under target/, so already gitignored
env_file="${STRAF3_ENV_FILE:-.env}"

site_host="127.0.0.1"
site_port="8787"
records_addr="${STRAF3_RECORDS_ADDR:-127.0.0.1:8788}"

# ── output ──────────────────────────────────────────────────────────────────

say()  { printf '%s\n' "$*"; }
step() { printf '\n==> %s\n' "$*"; }
warn() { printf '    !! %s\n' "$*"; }
ok()   { printf '    ok %s\n' "$*"; }

# ── the environment ─────────────────────────────────────────────────────────

# Read `.env` into this process's environment without ever echoing a value.
#
# Deliberately not `set -a; source .env`: sourcing executes the file, so a
# stray backtick in a credential would run as a command. This reads it as
# data — KEY=VALUE, one per line, `#` comments and blank lines skipped, and
# surrounding quotes stripped.
load_env() {
  if [ ! -f "$env_file" ]; then
    return 1
  fi
  local line key value
  while IFS= read -r line || [ -n "$line" ]; do
    case "$line" in ''|\#*) continue ;; esac
    key="${line%%=*}"
    value="${line#*=}"
    [ "$key" = "$line" ] && continue          # no '=' on the line
    key="$(printf '%s' "$key" | tr -d '[:space:]')"
    case "$value" in
      \"*\") value="${value#\"}"; value="${value%\"}" ;;
      \'*\') value="${value#\'}"; value="${value%\'}" ;;
    esac
    export "$key=$value"
  done < "$env_file"
  return 0
}

# Report presence, never contents.
report_env() {
  local name
  for name in DATABASE_URL NEON_AUTH_BASE_URL NEON_AUTH_JWKS_URL STRAF3_ORIGIN; do
    if [ -n "${!name:-}" ]; then
      ok "$name is set (${#name} char name, value not printed)"
    else
      warn "$name is NOT set"
    fi
  done
}

# ── process bookkeeping ─────────────────────────────────────────────────────
#
# Each process is started in its own session with `setsid`, so `down` can kill
# the whole tree by process group. That matters here more than usual: the
# service runs under `cargo run`, so killing the recorded pid would kill cargo
# and leave the binary it spawned holding port 8788 — the orphan this script
# exists to not produce.

start() {
  local name="$1"; shift
  local pidfile="$run_dir/$name.pid"
  local log="$run_dir/$name.log"

  if running "$name"; then
    ok "$name already running (pid $(cat "$pidfile"))"
    return 0
  fi
  mkdir -p "$run_dir"
  setsid "$@" >"$log" 2>&1 &
  local pid=$!
  echo "$pid" > "$pidfile"
  ok "$name started (pid $pid, log $log)"
}

running() {
  local pidfile="$run_dir/$1.pid"
  [ -f "$pidfile" ] || return 1
  local pid; pid="$(cat "$pidfile" 2>/dev/null)"
  [ -n "$pid" ] || return 1
  kill -0 "$pid" 2>/dev/null
}

stop() {
  local name="$1"
  local pidfile="$run_dir/$name.pid"
  [ -f "$pidfile" ] || { say "    $name: not running"; return 0; }
  local pid; pid="$(cat "$pidfile" 2>/dev/null)"
  if [ -n "$pid" ] && kill -0 "$pid" 2>/dev/null; then
    # Negative pid = the whole process group, which is what setsid bought.
    kill -TERM -- "-$pid" 2>/dev/null || kill -TERM "$pid" 2>/dev/null
    local n=0
    while kill -0 "$pid" 2>/dev/null && [ "$n" -lt 50 ]; do
      sleep 0.1; n=$((n + 1))
    done
    if kill -0 "$pid" 2>/dev/null; then
      kill -KILL -- "-$pid" 2>/dev/null || kill -KILL "$pid" 2>/dev/null
      say "    $name: killed (did not stop on TERM)"
    else
      say "    $name: stopped"
    fi
  else
    say "    $name: not running (stale pid file)"
  fi
  rm -f "$pidfile"
}

# Is anything listening on host:port? `ss` is in every WSL2 image; the curl
# fallback is there so this still answers on a box without it.
listening() {
  local hostport="$1"
  if command -v ss >/dev/null 2>&1; then
    ss -ltn 2>/dev/null | grep -q "[[:space:]]${hostport}[[:space:]]"
  else
    curl -s -o /dev/null --max-time 1 "http://${hostport}/" 2>/dev/null
  fi
}

wait_for_port() {
  local hostport="$1" name="$2" n=0
  while [ "$n" -lt 100 ]; do
    listening "$hostport" && { ok "$name is listening on $hostport"; return 0; }
    running "$name" || { warn "$name exited before it listened — see $run_dir/$name.log"; return 1; }
    sleep 0.2; n=$((n + 1))
  done
  warn "$name did not listen on $hostport within 20 s — see $run_dir/$name.log"
  return 1
}

# ── up ──────────────────────────────────────────────────────────────────────

up() {
  mkdir -p "$run_dir"

  step "environment"
  if load_env; then
    ok "read $env_file (gitignored; no value is printed or logged)"
  else
    warn "no $env_file at the repository root."
    warn "It is gitignored, so it does not follow a git worktree. Copy it from"
    warn "the main checkout. Without it the records service has no DATABASE_URL"
    warn "and will refuse to start; the site still comes up."
  fi
  report_env
  # The one origin. Stated rather than inferred, and checked against `.env` so
  # a disagreement is visible instead of producing a site nobody can reach.
  local origin="http://localhost:${site_port}"
  if [ -n "${STRAF3_ORIGIN:-}" ] && [ "${STRAF3_ORIGIN}" != "$origin" ]; then
    warn "STRAF3_ORIGIN in $env_file is not $origin — the site is served at $origin"
  fi

  step "the browser client bundle"
  if [ -x crates/straf3-game/web/build.sh ]; then
    if crates/straf3-game/web/build.sh >"$run_dir/client-build.log" 2>&1; then
      ok "built crates/straf3-game/web/pkg"
      grep -E '^\s+(straf3_game|TOTAL)' "$run_dir/client-build.log" | sed 's/^/    /'
    else
      warn "the client build failed — see $run_dir/client-build.log"
      tail -5 "$run_dir/client-build.log" | sed 's/^/    /'
    fi
  else
    warn "crates/straf3-game/web/build.sh is missing or not executable"
  fi

  step "the records service on $records_addr"
  if [ -d crates/straf3-records ]; then
    if [ -z "${DATABASE_URL:-}" ]; then
      warn "DATABASE_URL is not set, so the service is not started."
      warn "/v1/* will answer 503 no_records_service, which the site renders as"
      warn "unanswerable — a real state, not a broken one."
    else
      # Two binaries, one crate: contracts §E2. Building once up front means
      # the two `cargo run`s do not race each other for the build lock and
      # then look like slow start-ups.
      say "    building (first build takes minutes; output in $run_dir/records-build.log)"
      if cargo build -p straf3-records >"$run_dir/records-build.log" 2>&1; then
        ok "cargo build -p straf3-records"
        STRAF3_RECORDS_ADDR="$records_addr" start records-api \
          cargo run --quiet -p straf3-records --bin api
        STRAF3_RECORDS_ADDR="$records_addr" start records-verifier \
          cargo run --quiet -p straf3-records --bin verifier
        wait_for_port "$records_addr" records-api
      else
        warn "the records service does not build — see $run_dir/records-build.log"
        tail -15 "$run_dir/records-build.log" | sed 's/^/    /'
      fi
    fi
  else
    warn "crates/straf3-records does not exist in this tree yet — skipping."
    warn "/v1/* will answer 503 no_records_service."
  fi

  step "the site on ${site_host}:${site_port}"
  if [ -f web/dev/serve.mjs ]; then
    local api_args=()
    if listening "$records_addr"; then
      api_args=(--api "http://$records_addr")
    else
      warn "nothing is listening on $records_addr; starting the site without --api"
    fi
    start site node web/dev/serve.mjs \
      --port "$site_port" --host "$site_host" "${api_args[@]}"
    wait_for_port "${site_host}:${site_port}" site
  else
    warn "web/dev/serve.mjs is missing — there is no origin to serve"
  fi

  step "the origin"
  say "    $origin"
  say "    $origin/play/coil          play"
  say "    $origin/m/coil/cpm         a leaderboard"
  say "    $origin/watch/<digest16>   watch a record back"
  say ""
  say "    Chrome on a software-only host needs:"
  say "      --enable-unsafe-webgpu --use-angle=swiftshader"
  say ""
  say "    take it down with: tools/straf3-webcheck/ecosystem.sh down"
  status
}

# ── down ────────────────────────────────────────────────────────────────────

down() {
  step "stopping"
  stop site
  stop records-verifier
  stop records-api

  # A port still held after every recorded pid is gone is the orphan case, and
  # it is worth reporting loudly rather than discovering as "address in use"
  # on the next bring-up.
  local leftover=0
  for hp in "${site_host}:${site_port}" "$records_addr"; do
    if listening "$hp"; then
      warn "something is STILL listening on $hp and it is not ours"
      command -v ss >/dev/null 2>&1 && ss -ltnp 2>/dev/null | grep "$hp" | sed 's/^/    /'
      leftover=1
    fi
  done
  [ "$leftover" = 0 ] && ok "both ports are free"
  return 0
}

# ── status ──────────────────────────────────────────────────────────────────

status() {
  step "status"
  local name
  for name in site records-api records-verifier; do
    if running "$name"; then
      ok "$name running (pid $(cat "$run_dir/$name.pid"))"
    else
      say "    $name not running"
    fi
  done
  listening "${site_host}:${site_port}" \
    && ok "http://localhost:${site_port} answers" \
    || warn "nothing on ${site_host}:${site_port}"
  listening "$records_addr" \
    && ok "$records_addr answers" \
    || warn "nothing on $records_addr"
}

case "${1:-}" in
  up)      up ;;
  down)    down ;;
  status)  status ;;
  logs)    tail -n 40 -F "$run_dir"/*.log ;;
  *)
    sed -n '2,40p' "$0" | sed 's/^# \{0,1\}//'
    exit 1
    ;;
esac
