FROM rust:alpine3.20 AS builder

COPY Cargo.toml Cargo.lock /src/
RUN mkdir -p /src/src \
  && touch /src/src/main.rs \
  && cargo fetch --locked --manifest-path /src/Cargo.toml

ENV \
  # TODO: uncomment when libgit2 is at least version 1.8.1
  # LIBGIT2_NO_VENDOR=1 \
  OPENSSL_NO_VENDOR=1 \
  RUSTFLAGS='-C target-feature=-crt-static'
RUN apk add --no-cache --no-progress libgit2-dev musl-dev openssl-dev zlib-dev

COPY . /src/
RUN cargo install --locked --path /src --root /pkg \
  && strip /pkg/bin/pear-reviewer

################################################################################

FROM alpine:3.20

# guessed based on https://github.com/catthehacker/docker_images/blob/master/linux/ubuntu/scripts/runner.sh#L6-L12
RUN addgroup -g 1001 runner \
  && adduser -h /home/runner -s /bin/bash -G runner -D -u 1001 runner

# upgrade all installed packages to fix potential CVEs in advance
# also remove apk package manager to hopefully remove dependency on OpenSSL 🤞
RUN apk upgrade --no-cache --no-progress \
  && apk add --no-cache --no-progress libgcc libgit2 openssl zlib \
  && apk del --no-cache --no-progress apk-tools alpine-keys

COPY --from=builder /pkg/bin/pear-reviewer /usr/bin/pear-reviewer
# make sure the binary can be executed
RUN pear-reviewer --version 2>/dev/null

USER runner
ENTRYPOINT [ "/usr/bin/pear-reviewer" ]
