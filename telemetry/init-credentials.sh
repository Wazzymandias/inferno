#!/bin/sh
set -eu

password_file=/run/signoz-credentials/postgres-password
password=${SIGNOZ_POSTGRES_PASSWORD:-}

# SigNoz uses this value in a PostgreSQL URI without additional encoding.
case "$password" in
    *[!a-zA-Z0-9._~-]*)
        echo 'SIGNOZ_POSTGRES_PASSWORD must contain only URL-safe characters.' >&2
        exit 1
        ;;
esac

if [ -e "$password_file" ]; then
    if [ ! -s "$password_file" ]; then
        echo 'The stored SigNoz database password is empty.' >&2
        exit 1
    fi
    if [ -n "$password" ] && [ "$password" != "$(cat "$password_file")" ]; then
        echo 'SIGNOZ_POSTGRES_PASSWORD differs from the stored database password; keep the existing value.' >&2
        exit 1
    fi
    exit 0
fi

if [ -z "$password" ]; then
    password=$(openssl rand -hex 32)
fi

umask 077
temporary_file="$password_file.tmp.$$"
trap 'rm -f "$temporary_file"' EXIT
printf '%s' "$password" > "$temporary_file"
# PostgreSQL's entrypoint and SigNoz read the file through read-only mounts.
chmod 0444 "$temporary_file"
mv "$temporary_file" "$password_file"
unset password
