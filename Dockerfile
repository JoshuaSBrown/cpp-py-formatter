# Build the GitHub Action
FROM rust:1.52 as builder
WORKDIR /usr/src/myapp
COPY Cargo.toml .
COPY Cargo.lock .
COPY src ./src
RUN cargo install --path .

# GitHub Action Image
FROM ubuntu:20.04
# Install our apt packages
RUN apt-get update
RUN apt-get upgrade -y
RUN apt-get install -y git \
  python3-pip \
  clang-format-10 clang-format-11 clang-format-12 \
  && rm -rf /var/lib/apt/lists/*
RUN python3 -m pip install --upgrade pip
RUN python3 -m pip install black

COPY gitconfig /etc

COPY --from=builder /usr/local/cargo/bin/cpp-py-format /cpp-py-format
ENTRYPOINT [ "/cpp-py-format" ]
