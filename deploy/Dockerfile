# Debian-slim based
FROM rust:1.88-slim AS builder

# Install required build dependencies
RUN apt-get update && apt-get install -y \
    pkg-config \
    libssl-dev \
    git \
    make \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /fckloud

COPY .git ./.git/
COPY Cargo.toml Cargo.lock ./
COPY crates/ ./crates/
COPY Makefile ./

RUN make build

# ---------------------------------------------------------------------------- #

FROM alpine:3.20

LABEL \
    org.opencontainers.image.title="fckloud" \
    org.opencontainers.image.description="K8s controller for Node Public IPs (without relying on fucking clouds)" \
    org.opencontainers.image.vendor="Ilya Stroy <inaneverb@pm.me>" \
    org.opencontainers.image.source="https://github.com/inaneverb/fckloud" \
    org.opencontainers.image.licenses="MIT"

# Install runtime dependencies and tini (init system)
RUN apk add --no-cache \
    ca-certificates \
    tini \
    && addgroup -g 65532 -S nonroot \
    && adduser -u 65532 -S nonroot -G nonroot

COPY --from=builder /fckloud/target/release/fckloud /usr/local/bin/fckloud

RUN chown nonroot:nonroot /usr/local/bin/fckloud

USER nonroot

# Default command will be overridden in Kubernetes
ENTRYPOINT ["/sbin/tini", "--", "/usr/local/bin/fckloud", "run"]
CMD ["--help"]

# Env vars to configure the controller.
ENV FCKLOUD_NODE_NAME=""
ENV FCKLOUD_LOG_JSON="false"
ENV FCKLOUD_STRICT="false"
