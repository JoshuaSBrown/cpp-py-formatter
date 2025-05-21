# Build the GitHub Action
FROM rust:1.87.0 AS builder
WORKDIR /usr/src/myapp
COPY Cargo.toml .
COPY Cargo.lock .
COPY src ./src
RUN cargo install --path .

# GitHub Action Image
FROM ubuntu:24.04
# Install our apt packages
RUN apt update
RUN apt upgrade -y
RUN apt install -y git
RUN apt install -y python3-pip python3.12-venv

# Create a virtual environment
RUN python3 -m venv /opt/venv

# Activate the virtual environment and install packages
# This ensures pip installs are isolated to /opt/venv
ENV PATH="/opt/venv/bin:$PATH"

RUN python3 -m pip install --upgrade pip
RUN python3 -m pip install black

# Install clang-formats
ADD ./clang-format /clang-format
COPY --from=builder /usr/local/cargo/bin/cpp-py-format /cpp-py-format
ENTRYPOINT [ "/cpp-py-format" ]
