# Debian-slim based
FROM rust:1.88-alpine3.20 AS builder

# Install required build dependencies
RUN apk add --no-cache \
    pkgconfig \
    openssl-dev \
    openssl-libs-static \
    git \
    musl-dev \
    make

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

COPY --from=builder /fckloud/target/release/fckloud /usr/local/bin/

RUN chown nonroot:nonroot /usr/local/bin/fckloud

USER nonroot

# Default command will be overridden in Kubernetes
ENTRYPOINT ["/sbin/tini", "--", "/usr/local/bin/fckloud"]
CMD ["run", "--strict"]

# Env vars to configure the controller.
ENV FCKLOUD_NODE=""
ENV FCKLOUD_JSON="false"
ENV FCKLOUD_STRICT="false"
