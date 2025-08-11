ifeq ($(OS),Windows_NT)
    EXEEXT := .exe
else
    EXEEXT :=
endif

CARGO := cargo$(EXEEXT)
RUSTUP := rustup$(EXEEXT)

.PHONY: build run test clean lint fmt check install-tools debug-build debug-run

# Default target
all: build

# Release build (default)
build:
	$(CARGO) build --release

# Debug build
debug-build:
	$(CARGO) build

# Run in release mode (default)
run:
	$(CARGO) run --release

# Run in debug mode
debug-run:
	$(CARGO) run

# Run tests
test:
	$(CARGO) test --release

# Clean build artifacts
clean:
	$(CARGO) clean

# Install development tools
install-tools:
	$(RUSTUP) component add rustfmt clippy
	$(CARGO) install cargo-audit cargo-outdated

# Format code
fmt:
	$(CARGO) fmt

# Run clippy linter with strict settings
lint:
	$(CARGO) clippy 
		--all-targets \
		--all-features \
		-- \
	    -D warnings \
		-D clippy::all \
		-D clippy::pedantic \
		-A clippy::module_name_repetitions \
		-A clippy::too_many_lines

# Security audit
audit:
	$(CARGO) audit

# Check for outdated dependencies
outdated:
	$(CARGO) outdated

# Full check pipeline (format, lint, test)
check: fmt lint test

# Development workflow
dev: debug-build debug-run

# CI/Production workflow
ci: check audit build
