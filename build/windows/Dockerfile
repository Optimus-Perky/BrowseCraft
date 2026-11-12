FROM chromium-builder:trixie-slim
USER root
RUN apt-get update && apt-get install -y --no-install-recommends fuse3 libfuse2t64 libattr1 && rm -rf /var/lib/apt/lists/*
RUN echo user_allow_other >> /etc/fuse.conf
USER builder
