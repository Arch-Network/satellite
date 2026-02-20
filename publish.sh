#!/bin/bash

# Script to publish all Satellite workspace crates to crates.io in dependency order
# Excludes arch-satellite-borsh-derive-internal (has fixed version)

# Don't use set -e since we want to continue on errors and just warn

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

# Function to print colored output
info() {
    echo -e "${BLUE}ℹ${NC} $1"
}

success() {
    echo -e "${GREEN}✓${NC} $1"
}

error() {
    echo -e "${RED}✗${NC} $1"
}

warn() {
    echo -e "${YELLOW}⚠${NC} $1"
}

# Function to publish a crate
publish_crate() {
    local crate_name=$1
    
    info "Publishing ${crate_name}..."
    
    # Use cargo publish -p from workspace root instead of changing directories
    # First check if dry-run works
    local dry_run_output
    dry_run_output=$(cargo publish -p "$crate_name" --dry-run 2>&1)
    local dry_run_exit=$?
    
    if [ $dry_run_exit -eq 0 ]; then
        # Dry-run succeeded, try actual publish
        local publish_output
        publish_output=$(cargo publish -p "$crate_name" 2>&1)
        local publish_exit=$?
        
        if [ $publish_exit -eq 0 ]; then
            success "Published ${crate_name}"
            # Wait a bit for crates.io to index
            sleep 3
        else
            # Check if it's because it's already published
            if echo "$publish_output" | grep -q "already exists"; then
                warn "${crate_name} already published, skipping..."
            else
                error "Failed to publish ${crate_name}"
                echo "$publish_output" | head -5
            fi
            return 1
        fi
    else
        # Check if it's because it's already published
        if echo "$dry_run_output" | grep -q "already exists"; then
            warn "${crate_name} already published, skipping..."
        else
            warn "Dry-run failed for ${crate_name}, skipping..."
        fi
        return 1
    fi
}

# Check if we're in the workspace root
if [ ! -f "Cargo.toml" ] || ! grep -q "\[workspace\]" Cargo.toml; then
    error "Must be run from the workspace root directory"
    exit 1
fi

info "Starting publication process..."
CURRENT_VERSION=$(grep -A 2 '\[workspace.package\]' Cargo.toml | grep '^version = ' | sed 's/.*version = "\(.*\)"/\1/')
info "Current workspace version: ${CURRENT_VERSION}"
echo ""


# Tier 1: No workspace dependencies
info "=== Tier 1: Publishing crates with no workspace dependencies ==="
publish_crate "arch-satellite-serde" || warn "arch-satellite-serde failed or already published"
publish_crate "arch-satellite-math" || warn "arch-satellite-math failed or already published"
publish_crate "arch-satellite-collections" || warn "arch-satellite-collections failed or already published"
publish_crate "arch-satellite-lang-idl-spec" || warn "arch-satellite-lang-idl-spec failed or already published"
publish_crate "arch-satellite-syn" || warn "arch-satellite-syn failed or already published"

echo ""

# Tier 2: Depend on Tier 1
info "=== Tier 2: Publishing crates that depend on Tier 1 ==="
publish_crate "arch-satellite-bitcoin-transactions" || warn "arch-satellite-bitcoin-transactions failed or already published"

# Publish all attribute crates
info "Publishing lang attribute crates..."
publish_crate "arch-satellite-attribute-access-control" || warn "arch-satellite-attribute-access-control failed or already published"
publish_crate "arch-satellite-attribute-account" || warn "arch-satellite-attribute-account failed or already published"
publish_crate "arch-satellite-attribute-constant" || warn "arch-satellite-attribute-constant failed or already published"
publish_crate "arch-satellite-attribute-error" || warn "arch-satellite-attribute-error failed or already published"
publish_crate "arch-satellite-attribute-event" || warn "arch-satellite-attribute-event failed or already published"
publish_crate "arch-satellite-attribute-program" || warn "arch-satellite-attribute-program failed or already published"

# Publish all derive crates
info "Publishing lang derive crates..."
publish_crate "arch-satellite-derive-accounts" || warn "arch-satellite-derive-accounts failed or already published"
publish_crate "arch-satellite-derive-serde" || warn "arch-satellite-derive-serde failed or already published"
publish_crate "arch-satellite-derive-space" || warn "arch-satellite-derive-space failed or already published"

echo ""

# Tier 3: Depend on Tier 2
info "=== Tier 3: Publishing crates that depend on Tier 2 ==="
publish_crate "arch-satellite-bitcoin" || warn "arch-satellite-bitcoin failed or already published"
publish_crate "arch-satellite-lang-idl" || warn "arch-satellite-lang-idl failed or already published"
publish_crate "arch-satellite-lang" || warn "arch-satellite-lang failed or already published"

echo ""

# Tier 4: Depend on Tier 3
info "=== Tier 4: Publishing crates that depend on Tier 3 ==="
publish_crate "arch-satellite-shard" || warn "arch-satellite-shard failed or already published"
# publish_crate "anchor-client" || warn "anchor-client failed or already published"
publish_crate "arch-satellite-apl" || warn "arch-satellite-apl failed or already published"
# publish_crate "avm" || warn "avm failed or already published"

echo ""

# Tier 5: Depend on Tier 4
info "=== Tier 5: Publishing crates that depend on Tier 4 ==="
# publish_crate "anchor-cli" || warn "anchor-cli failed or already published"

echo ""
success "Publication process completed!"
info "Note: Some crates may have been skipped if they were already published or failed validation."
info "Check the output above for any warnings or errors."

