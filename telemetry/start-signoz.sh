#!/bin/sh
set -eu

if [ -z "${SIGNOZ_SQLSTORE_POSTGRES_DSN:-}" ]; then
    password=$(cat /run/signoz-credentials/postgres-password)
    export SIGNOZ_SQLSTORE_POSTGRES_DSN="postgres://signoz:${password}@signoz-postgres:5432/signoz?sslmode=disable"
    unset password
fi

exec ./signoz server "$@"
