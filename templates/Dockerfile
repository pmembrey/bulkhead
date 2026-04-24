FROM mcr.microsoft.com/devcontainers/base:ubuntu24.04@sha256:7ee7da33a68d997971660d91ecc8372e55a38a777c3c6bd6808daf91928052db

ARG TZ
ARG BULKHEAD_REMOTE_USER=vscode
ENV TZ="${TZ}" \
    RUSTUP_HOME=/usr/local/rustup \
    CARGO_HOME=/usr/local/cargo \
    PATH=/usr/local/cargo/bin:${PATH}

ARG BULKHEAD_RUST_TOOLCHAIN=1.95.0
ARG BULKHEAD_ZELLIJ_VERSION=0.44.1

SHELL ["/bin/bash", "-o", "pipefail", "-c"]

RUN apt-get update && apt-get install -y --no-install-recommends \
    bubblewrap \
    build-essential \
    curl \
    dnsutils \
    fd-find \
    fzf \
    iproute2 \
    ipset \
    iptables \
    jq \
    nano \
    ripgrep \
    socat \
    tmux \
    unzip \
    vim \
    zsh \
    && apt-get clean && rm -rf /var/lib/apt/lists/*

RUN curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | \
    sh -s -- -y --profile minimal --default-toolchain "${BULKHEAD_RUST_TOOLCHAIN}" --no-modify-path && \
    cargo install --locked zellij --version "${BULKHEAD_ZELLIJ_VERSION}" && \
    rustup component add rustfmt clippy && \
    chmod -R a+rwX /usr/local/rustup /usr/local/cargo

RUN if [ "${BULKHEAD_REMOTE_USER}" != "root" ] && [ "${BULKHEAD_REMOTE_USER}" != "vscode" ]; then \
        groupmod --new-name "${BULKHEAD_REMOTE_USER}" vscode && \
        usermod --login "${BULKHEAD_REMOTE_USER}" --home "/home/${BULKHEAD_REMOTE_USER}" --move-home -d "/home/${BULKHEAD_REMOTE_USER}" -s /bin/bash vscode && \
        mv /etc/sudoers.d/vscode "/etc/sudoers.d/${BULKHEAD_REMOTE_USER}" && \
        sed -i "s/^vscode /${BULKHEAD_REMOTE_USER} /" "/etc/sudoers.d/${BULKHEAD_REMOTE_USER}"; \
    fi && \
    mkdir -p /commandhistory /workspace && \
    touch /commandhistory/.bash_history && \
    if [ "${BULKHEAD_REMOTE_USER}" = "root" ]; then \
        chown -R root:root /commandhistory /workspace; \
    else \
        chown -R "${BULKHEAD_REMOTE_USER}:${BULKHEAD_REMOTE_USER}" /commandhistory /workspace; \
    fi

ENV DEVCONTAINER=true \
    SHELL=/bin/bash \
    EDITOR=vim \
    VISUAL=vim \
    HISTFILE=/commandhistory/.bash_history

WORKDIR /workspace
USER ${BULKHEAD_REMOTE_USER}
