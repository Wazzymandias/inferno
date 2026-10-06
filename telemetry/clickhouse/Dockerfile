# Bake SigNoz's histogram function into the release image instead of downloading
# executable dependencies at container startup. Compose pins the base image.
ARG CLICKHOUSE_IMAGE=clickhouse/clickhouse-server:25.12.5@sha256:cacf32d6884291dc2ff5e0156a97f46fc53ff7c929a7906d114e268a929dfd3a
FROM ${CLICKHOUSE_IMAGE}

ARG TARGETARCH
RUN set -eu; \
    case "${TARGETARCH}" in \
      amd64) checksum=33997073eb6d82b7be4f27f9fc8ec9e28a395e0f20674d5d54bb2fa28a75488d ;; \
      arm64) checksum=e5605ebffa82a450ebbcdf6cf19dad546e1e40d52dbf3e03cfd2d2d5b7394211 ;; \
      *) echo "Unsupported histogram function architecture: ${TARGETARCH}" >&2; exit 1 ;; \
    esac; \
    wget -q -O /tmp/histogram-quantile.tar.gz \
      "https://github.com/SigNoz/signoz/releases/download/histogram-quantile/v0.0.1/histogram-quantile_linux_${TARGETARCH}.tar.gz"; \
    printf '%s  %s\n' "${checksum}" /tmp/histogram-quantile.tar.gz | sha256sum -c -; \
    tar -xzf /tmp/histogram-quantile.tar.gz -C /tmp; \
    mkdir -p /opt/signoz/bin; \
    mv /tmp/histogram-quantile /opt/signoz/bin/histogramQuantile; \
    chmod 755 /opt/signoz/bin/histogramQuantile; \
    rm /tmp/histogram-quantile.tar.gz
