FROM --platform=$BUILDPLATFORM rust:1.87-bookworm AS builder

ARG TARGETARCH

# Set up cross-compilation
WORKDIR /.vars
RUN case "$TARGETARCH" in \
    "amd64") \
        printf "x86_64-unknown-linux-gnu" > target; \
        printf "gcc-x86-64-linux-gnu g++-x86-64-linux-gnu libc6-dev-amd64-cross" > deps; \
        echo '[target.x86_64-unknown-linux-gnu]' >> $CARGO_HOME/config.toml; \
        echo 'linker = "x86_64-linux-gnu-gcc"' >> $CARGO_HOME/config.toml ;; \
    "arm64") \
        printf "aarch64-unknown-linux-gnu" > target; \
        printf "gcc-aarch64-linux-gnu g++-aarch64-linux-gnu libc6-dev-arm64-cross" > deps; \
        echo '[target.aarch64-unknown-linux-gnu]' >> $CARGO_HOME/config.toml; \
        echo 'linker = "aarch64-linux-gnu-gcc"' >> $CARGO_HOME/config.toml ;; \
    *) echo "Unsupported architecture: $TARGETARCH" >&2 && exit 1 ;; \
esac

WORKDIR /dist
WORKDIR /build

# Install build dependencies
RUN --mount=type=cache,target=/var/lib/apt/lists,sharing=locked \
    --mount=type=cache,target=/var/cache/apt,sharing=locked \
    apt-get update \
    && apt-get install --no-install-recommends --yes \
        $(cat /.vars/deps) \
        build-essential \
        libclang-dev

# Add target to rustup and install rustfmt
RUN rustup target add $(cat /.vars/target) && \
    rustup component add rustfmt

# Build dependencies
COPY ./Cargo.toml ./Cargo.lock ./
COPY ./macros ./macros
RUN --mount=type=cache,target=/usr/local/cargo/registry,sharing=locked \
    --mount=type=cache,target=/usr/local/cargo/git,sharing=locked \
    --mount=type=tmpfs,target=/build/src \
    printf "#[allow(dead_code)]\nfn main() {}\n" > src/lib.rs && \
    cargo build --release --target=$(cat /.vars/target)

# Build source
COPY ./src ./src
RUN --mount=type=cache,target=/usr/local/cargo/registry,sharing=locked \
    --mount=type=cache,target=/usr/local/cargo/git,sharing=locked \
    cargo build --verbose --release --target=$(cat /.vars/target) && \
    mv /build/target/$(cat /.vars/target)/release/maestro-symphony /dist/maestro-symphony

# ---
FROM debian:bookworm-slim

LABEL org.opencontainers.image.source=https://github.com/maestro-org/maestro-symphony

ENV DEBCONF_NONINTERACTIVE_SEEN=true
ENV DEBIAN_FRONTEND=noninteractive

# Install system dependencies
RUN apt-get update \
    && apt-get install --no-install-recommends --yes \
        ca-certificates \
        tini \
    && apt-get clean \
    && rm -rf /var/lib/apt/lists/*

# Create nonroot user
RUN groupadd --gid 60000 nonroot \
    && useradd --no-log-init --create-home \
        --uid 60000 \
        --gid 60000 \
        --shell /sbin/nologin \
        nonroot

# Copy binary
COPY --from=builder /dist/maestro-symphony /usr/local/bin/maestro-symphony

USER nonroot

# Set entrypoint
ENTRYPOINT [ "/usr/bin/tini", "-g", "--", "/usr/local/bin/maestro-symphony" ]
