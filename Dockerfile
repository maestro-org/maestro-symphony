FROM --platform=$BUILDPLATFORM rust:1.87-bookworm AS builder

ARG TARGETARCH

WORKDIR /dist
WORKDIR /build

# Set up cross-compilation
RUN case "$TARGETARCH" in \
    "amd64") \
        printf "x86_64-unknown-linux-gnu" > .target; \
        printf "gcc-x86-64-linux-gnu" > .compiler; \
        ;; \
    "arm64") \
        printf "aarch64-unknown-linux-gnu" > .target; \
        printf "gcc-aarch64-linux-gnu" > .compiler; \
        ;; \
    *) echo "Unsupported architecture: $TARGETARCH" >&2 && exit 1 ;; \
esac

# Install build dependencies
RUN apt-get update \
    && apt-get install --no-install-recommends --yes \
        build-essential \
        libclang-dev \
        $(cat .compiler) \
    && apt-get clean \
    && rm -rf /var/lib/apt/lists/*

# Add target to rustup
RUN rustup target add $(cat .target)

# Build dependencies
COPY ./Cargo.toml ./Cargo.lock ./
COPY ./macros ./macros
RUN --mount=type=tmpfs,target=/build/src \
    printf "#[allow(dead_code)]\nfn main() {}\n" > src/lib.rs && \
    cargo build --release --target=$(cat .target)

# Build source
COPY ./src ./src
RUN cargo build --verbose --release --target=$(cat .target) && \
    mv /build/target/$(cat .target)/release/maestro-symphony /dist/maestro-symphony


FROM debian:bookworm-slim

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
RUN groupadd --gid 65532 nonroot \
    && useradd --no-log-init --create-home \
        --uid 65532 \
        --gid 65532 \
        --shell /sbin/nologin \
        nonroot

# Copy binary
COPY --from=builder /dist/maestro-symphony /usr/local/bin/maestro-symphony

USER nonroot

# Set entrypoint
ENTRYPOINT [ "/usr/bin/tini", "-g", "--", "/usr/local/bin/maestro-symphony" ]
