FROM rust:alpine AS builder

# git is here for vergen, which stamps the binary with the commit it came from.
RUN apk add --no-cache git build-base

WORKDIR /fckloud

# The whole tree, not a hand-picked subset: vergen asks git whether the working
# tree is clean, and a partial copy looks to git exactly like mass deletion.
COPY . .

RUN cargo build --release --locked

# ---------------------------------------------------------------------------- #

FROM alpine:3

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
