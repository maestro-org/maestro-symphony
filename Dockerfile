FROM --platform=$BUILDPLATFORM rust:1.87-bookworm AS builder

ARG TARGETARCH

ENV CC_aarch64_unknown_linux_gnu=aarch64-linux-gnu-gcc \
    CXX_aarch64_unknown_linux_gnu=aarch64-linux-gnu-g++ \
    AR_aarch64_unknown_linux_gnu=aarch64-linux-gnu-ar \
    CARGO_TARGET_AARCH64_UNKNOWN_LINUX_GNU_LINKER=aarch64-linux-gnu-gcc \
    CC_x86_64_unknown_linux_gnu=x86_64-linux-gnu-gcc \
    CXX_x86_64_unknown_linux_gnu=x86_64-linux-gnu-g++ \
    AR_x86_64_unknown_linux_gnu=x86_64-linux-gnu-ar \
    CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_LINKER=x86_64-linux-gnu-gcc

# Install build dependencies
RUN apt-get update \
    && apt-get install --no-install-recommends --yes \
        build-essential \
        gcc-aarch64-linux-gnu \
        gcc-x86-64-linux-gnu \
        libclang-dev \
    && apt-get clean \
    && rm -rf /var/lib/apt/lists/*

# Set up cross-compilation
RUN case "$TARGETARCH" in \
        "amd64") rustup target add x86_64-unknown-linux-gnu ;; \
        "arm64") rustup target add aarch64-unknown-linux-gnu ;; \
        *) echo "Unsupported architecture: $TARGETARCH" >&2 && exit 1 ;; \
    esac

WORKDIR /dist
WORKDIR /build

# Build dependencies
COPY ./Cargo.toml ./Cargo.lock ./
COPY ./macros ./macros
RUN --mount=type=tmpfs,target=/build/src \
    printf "#[allow(dead_code)]\nfn main() {}\n" > src/lib.rs && \
    case "$TARGETARCH" in \
        "amd64") TARGET="x86_64-unknown-linux-gnu" ;; \
        "arm64") TARGET="aarch64-unknown-linux-gnu" ;; \
    esac && \
    cargo build --release --target=$TARGET

# Build source
COPY ./src ./src
RUN case "$TARGETARCH" in \
        "amd64") TARGET="x86_64-unknown-linux-gnu" ;; \
        "arm64") TARGET="aarch64-unknown-linux-gnu" ;; \
        *) echo "Unsupported architecture: $TARGETARCH" >&2 && exit 1 ;; \
    esac && \
    cargo build --verbose --release --target=$TARGET && \
    mv /build/target/$TARGET/release/maestro-symphony /dist/maestro-symphony


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
COPY --from=builder /dist/maestro-symphony /bin/maestro-symphony

USER nonroot

# Set entrypoint
ENTRYPOINT ["/sbin/tini", "--", "/bin/maestro-symphony"]
