#!/usr/bin/env bash
#
# setup.sh — james-shell development environment setup
#
# Checks prerequisites, installs missing tools, and verifies the build.
#
# Usage:
#   ./setup.sh           # Full setup (install missing tools + build)
#   ./setup.sh --check   # Check-only mode (no installs, just report)
#

set -euo pipefail

# ---------------------------------------------------------------------------
# Configuration
# ---------------------------------------------------------------------------
REQUIRED_RUST_MAJOR=1
REQUIRED_RUST_MINOR=85
CHECK_ONLY=false

if [[ "${1:-}" == "--check" ]]; then
    CHECK_ONLY=true
fi

# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[0;33m'
BOLD='\033[1m'
RESET='\033[0m'

ok()   { printf "${GREEN}[OK]${RESET}    %s\n" "$1"; }
warn() { printf "${YELLOW}[WARN]${RESET}  %s\n" "$1"; }
fail() { printf "${RED}[FAIL]${RESET}  %s\n" "$1"; }
info() { printf "${BOLD}>>>${RESET} %s\n" "$1"; }

ERRORS=0

# ---------------------------------------------------------------------------
# Check: Git
# ---------------------------------------------------------------------------
check_git() {
    info "Checking for Git..."
    if command -v git &>/dev/null; then
        local version
        version=$(git --version | sed 's/git version //')
        ok "Git $version"
    else
        fail "Git is not installed."
        echo "    Install it from https://git-scm.com or via your package manager."
        ERRORS=$((ERRORS + 1))
    fi
}

# ---------------------------------------------------------------------------
# Check / Install: Rust (via rustup)
# ---------------------------------------------------------------------------
check_rust() {
    info "Checking for Rust toolchain..."

    if ! command -v rustup &>/dev/null; then
        fail "rustup is not installed."
        if $CHECK_ONLY; then
            echo "    Install it from https://rustup.rs"
            ERRORS=$((ERRORS + 1))
            return
        fi

        echo ""
        read -rp "    Install Rust via rustup now? [Y/n] " answer
        case "${answer:-Y}" in
            [Yy]*)
                curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
                # shellcheck disable=SC1091
                source "$HOME/.cargo/env"
                ;;
            *)
                echo "    Skipping Rust install. You will need it to build the project."
                ERRORS=$((ERRORS + 1))
                return
                ;;
        esac
    fi

    # Verify rustc version
    if command -v rustc &>/dev/null; then
        local version
        version=$(rustc --version | awk '{print $2}')
        local major minor
        major=$(echo "$version" | cut -d. -f1)
        minor=$(echo "$version" | cut -d. -f2)

        if [[ "$major" -gt "$REQUIRED_RUST_MAJOR" ]] || \
           { [[ "$major" -eq "$REQUIRED_RUST_MAJOR" ]] && [[ "$minor" -ge "$REQUIRED_RUST_MINOR" ]]; }; then
            ok "Rust $version (>= $REQUIRED_RUST_MAJOR.$REQUIRED_RUST_MINOR required)"
        else
            warn "Rust $version found, but >= $REQUIRED_RUST_MAJOR.$REQUIRED_RUST_MINOR is required."
            if ! $CHECK_ONLY; then
                echo "    Updating..."
                rustup update stable
            else
                echo "    Run: rustup update stable"
            fi
        fi
    else
        fail "rustc not found even though rustup is installed."
        echo "    Run: rustup install stable"
        ERRORS=$((ERRORS + 1))
    fi

    # Check cargo
    if command -v cargo &>/dev/null; then
        ok "Cargo $(cargo --version | awk '{print $2}')"
    else
        fail "Cargo not found."
        ERRORS=$((ERRORS + 1))
    fi
}

# ---------------------------------------------------------------------------
# Check / Install: Rust components (clippy, rustfmt)
# ---------------------------------------------------------------------------
check_components() {
    info "Checking Rust components..."

    for component in clippy rustfmt; do
        if rustup component list --installed 2>/dev/null | grep -q "$component"; then
            ok "$component"
        else
            warn "$component is not installed."
            if ! $CHECK_ONLY; then
                echo "    Installing $component..."
                rustup component add "$component"
                ok "$component installed"
            else
                echo "    Run: rustup component add $component"
            fi
        fi
    done
}

# ---------------------------------------------------------------------------
# Check / Install: Optional cargo subcommands
# ---------------------------------------------------------------------------
check_cargo_tools() {
    info "Checking optional cargo tools..."

    declare -A tools
    tools=(
        [cargo-watch]="cargo install cargo-watch"
        [cargo-audit]="cargo install cargo-audit"
    )

    for tool in "${!tools[@]}"; do
        # cargo subcommands are invoked as "cargo watch", binary is "cargo-watch"
        if command -v "$tool" &>/dev/null || cargo --list 2>/dev/null | grep -q "${tool#cargo-}"; then
            ok "$tool"
        else
            warn "$tool is not installed (optional)."
            if ! $CHECK_ONLY; then
                read -rp "    Install $tool now? [Y/n] " answer
                case "${answer:-Y}" in
                    [Yy]*)
                        ${tools[$tool]}
                        ok "$tool installed"
                        ;;
                    *)
                        echo "    Skipped."
                        ;;
                esac
            else
                echo "    Run: ${tools[$tool]}"
            fi
        fi
    done
}

# ---------------------------------------------------------------------------
# Verify: Build the project
# ---------------------------------------------------------------------------
verify_build() {
    info "Building the project..."

    if cargo build 2>&1; then
        ok "cargo build succeeded"
    else
        fail "cargo build failed. See errors above."
        ERRORS=$((ERRORS + 1))
    fi
}

# ---------------------------------------------------------------------------
# Verify: Run tests
# ---------------------------------------------------------------------------
verify_tests() {
    info "Running tests..."

    if cargo test 2>&1; then
        ok "cargo test passed"
    else
        fail "cargo test failed. See errors above."
        ERRORS=$((ERRORS + 1))
    fi
}

# ---------------------------------------------------------------------------
# Main
# ---------------------------------------------------------------------------
main() {
    echo ""
    echo "============================================"
    if $CHECK_ONLY; then
        echo "  james-shell Environment Check"
    else
        echo "  james-shell Development Setup"
    fi
    echo "============================================"
    echo ""

    check_git
    echo ""
    check_rust
    echo ""
    check_components
    echo ""
    check_cargo_tools
    echo ""

    if ! $CHECK_ONLY; then
        verify_build
        echo ""
        verify_tests
        echo ""
    fi

    echo "============================================"
    if [[ "$ERRORS" -eq 0 ]]; then
        printf "${GREEN}${BOLD}  All checks passed.${RESET}\n"
        if ! $CHECK_ONLY; then
            echo ""
            echo "  You're ready to go! Start with:"
            echo "    cargo run"
            echo ""
            echo "  Or start learning with:"
            echo "    docs/module-00-foundations.md"
        fi
    else
        printf "${RED}${BOLD}  $ERRORS issue(s) found.${RESET}\n"
        echo "  Fix the issues above and re-run this script."
    fi
    echo "============================================"
    echo ""

    exit "$ERRORS"
}

main
